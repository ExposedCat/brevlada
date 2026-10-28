use super::{Storage, messages, messages::Mailbox};
use crate::{
    backend::parser::normalize_message_id,
    models::{Message, SentMessage, conversation, subject_key},
};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::{HashMap, HashSet};

pub(super) fn keys(message: &Message) -> Vec<(u8, String)> {
    let mut keys: HashSet<_> = std::iter::once(&message.message_id)
        .chain(&message.references)
        .chain(&message.in_reply_to)
        .map(|id| normalize_message_id(id))
        .filter(|id| !id.is_empty())
        .map(|id| (1, id.to_owned()))
        .collect();
    let subject = subject_key(&message.subject);
    if !subject.is_empty() {
        keys.insert((0, subject));
    }
    keys.into_iter().collect()
}

pub(super) fn index(
    connection: &Connection,
    id: i64,
    account: &str,
    message: &Message,
) -> Result<()> {
    connection
        .prepare_cached("DELETE FROM rust_conversation_keys WHERE header_id=?1")?
        .execute([id])?;
    let mut insert =
        connection.prepare_cached("INSERT INTO rust_conversation_keys VALUES (?1,?2,?3,?4)")?;
    for (kind, value) in keys(message) {
        insert.execute(params![id, account, kind, value])?;
    }
    Ok(())
}

pub(super) fn reindex(connection: &Connection, id: i64, message: &Message) -> Result<()> {
    let account: String = connection.query_row(
        "SELECT account_id FROM rust_headers WHERE id=?1",
        [id],
        |row| row.get(0),
    )?;
    index(connection, id, &account, message)
}

pub(super) fn migrate(transaction: &Transaction<'_>) -> Result<()> {
    let indexed: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM rust_metadata WHERE key='conversation_keys')",
        [],
        |row| row.get(0),
    )?;
    if indexed {
        return Ok(());
    }
    let mut statement = transaction.prepare(
        "SELECT id, account_id, message_id, subject, in_reply_to, \"references\" FROM rust_headers",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        index(
            transaction,
            row.get(0)?,
            row.get_ref(1)?.as_str()?,
            &Message {
                message_id: row.get(2)?,
                subject: row.get(3)?,
                in_reply_to: row.get(4)?,
                references: messages::json(row, 5)?,
                ..Default::default()
            },
        )?;
    }
    transaction.execute(
        "INSERT INTO rust_metadata VALUES ('conversation_keys','1')",
        [],
    )?;
    Ok(())
}

impl Storage {
    #[cfg(test)]
    pub fn related_sent(
        &self,
        account: &str,
        folder: &str,
        received: &[Message],
    ) -> Result<Vec<SentMessage>> {
        self.related_messages(account, folder, received, false)
    }

    pub fn related_messages(
        &self,
        account: &str,
        folder: &str,
        received: &[Message],
        outgoing: bool,
    ) -> Result<Vec<SentMessage>> {
        let mut candidates = HashMap::new();
        let mut queried = HashSet::new();
        let mut pending: Vec<_> = received.iter().flat_map(keys).collect();
        let mut statement = self.0.prepare_cached(concat!(
            messages::select!("'', '', '[]', coalesce(h.folder, ''), h.id"),
            " WHERE h.id IN (SELECT header_id FROM rust_conversation_keys WHERE account_id=?1 AND kind=?2 AND value=?3)
              AND (h.folder IS NULL OR h.folder<>?4 OR NOT h.is_draft)
              ORDER BY h.folder IS NOT NULL, h.timestamp DESC, h.folder, h.uid DESC"
        ))?;
        loop {
            for (kind, value) in pending.drain(..) {
                if !queried.insert((kind, value.clone())) {
                    continue;
                }
                for row in statement.query_map(params![account, kind, value, folder], |row| {
                    Ok((
                        row.get::<_, i64>(30)?,
                        SentMessage {
                            folder: row.get(29)?,
                            message: messages::decode(row)?,
                        },
                    ))
                })? {
                    let (id, mut candidate) = row?;
                    if candidate.message.is_spam
                        || super::super::mail_sync::excluded_folder(&candidate.folder, &[])
                    {
                        continue;
                    }
                    candidate.message.body_loaded = false;
                    if !candidate.folder.is_empty() {
                        candidate.message.uid_validity = candidate
                            .message
                            .uid_validity
                            .or(self.validity(account, &candidate.folder)?);
                    }
                    super::super::mail_sync::ensure_draft_id(
                        &mut candidate.message,
                        &candidate.folder,
                    );
                    candidates.entry(id).or_insert(candidate);
                }
            }
            let mut ordered: Vec<_> = candidates.values().cloned().collect();
            ordered.sort_by_key(|item| {
                (
                    !item.folder.is_empty(),
                    std::cmp::Reverse(item.message.timestamp),
                    item.folder.clone(),
                    std::cmp::Reverse(item.message.uid),
                )
            });
            let mut related = conversation::related_direction(received, &ordered, outgoing);
            pending = related
                .iter()
                .flat_map(|item| keys(&item.message))
                .filter(|key| !queried.contains(key))
                .collect();
            if !pending.is_empty() {
                continue;
            }
            for (_, item) in related
                .iter_mut()
                .enumerate()
                .filter(|(index, item)| *index < 25 || item.folder.is_empty())
            {
                let loaded = if item.folder.is_empty() {
                    self.0
                        .prepare_cached(concat!(
                            messages::select!(),
                            " WHERE h.account_id=?1 AND h.folder IS NULL AND h.message_id=?2"
                        ))?
                        .query_row(params![account, item.message.message_id], messages::decode)
                        .optional()?
                } else {
                    self.message(account, &item.folder, item.message.uid)?
                };
                if let Some(mut message) = loaded {
                    message.is_draft |= item.message.is_draft;
                    message.uid_validity = message.uid_validity.or(item.message.uid_validity);
                    super::super::mail_sync::ensure_draft_id(&mut message, &item.folder);
                    item.message = message;
                }
            }
            return Ok(related);
        }
    }

    pub(in crate::backend) fn missing_conversation_uids(
        &self,
        account: &str,
        folder: &str,
        validity: u32,
        uids: &[u32],
    ) -> Result<Vec<u32>> {
        if self.validity(account, folder)? != Some(validity) {
            return Ok(uids.to_vec());
        }
        let Some(mailbox) = Mailbox::find(&self.0, account, folder)? else {
            return Ok(uids.to_vec());
        };
        let mut statement = self.0.prepare_cached("SELECT value FROM json_each(?1) WHERE NOT EXISTS(SELECT 1 FROM rust_headers WHERE id=(?2 | value)) ORDER BY value DESC")?;
        Ok(statement
            .query_map(
                params![serde_json::to_string(uids)?, mailbox.first()],
                |row| row.get(0),
            )?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub(in crate::backend) fn store_conversation_headers(
        &self,
        account: &str,
        folder: &str,
        expected: Option<u32>,
        validity: u32,
        headers: &[Message],
    ) -> Result<bool> {
        self.write(|transaction| {
            if Self::folder_validity(transaction, account, folder)? != expected {
                return Ok(false);
            }
            let mailbox = Mailbox::ensure(transaction, account, folder)?;
            if expected != Some(validity) {
                transaction.execute(
                    "DELETE FROM rust_headers WHERE id BETWEEN ?1 AND ?2",
                    params![mailbox.first(), mailbox.last()],
                )?;
                transaction.execute(
                    "INSERT OR REPLACE INTO rust_folders VALUES (?1,?2,?3)",
                    params![account, folder, validity],
                )?;
            }
            let removed = Self::removed_in(transaction, account, folder, validity)?;
            let revision = Self::next_read_revision(transaction)?;
            for message in headers
                .iter()
                .filter(|message| !removed.contains(&message.uid))
            {
                let mut message = message.clone();
                message.uid_validity = Some(validity);
                message.read_revision = revision;
                message.flag_revision = revision;
                messages::add(
                    transaction,
                    mailbox.id(message.uid),
                    account,
                    Some(folder),
                    &message,
                )?;
            }
            Ok(true)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(uid: u32, id: &str, sender: &str, references: &[&str]) -> Message {
        Message {
            uid,
            message_id: id.into(),
            timestamp: uid as i64,
            sender: sender.into(),
            recipients: if sender == "me@example.com" {
                "salon@example.com"
            } else {
                "me@example.com"
            }
            .into(),
            subject: "Re: Woman Haircut Appointment with Mark".into(),
            references: references.iter().map(|id| (*id).into()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn reconstructs_archived_history_when_the_latest_reply_has_no_references() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let latest = message(10, "confirmed", "salon@example.com", &[]);
        let reply = message(9, "accepted", "me@example.com", &["offer"]);
        storage.store("me@example.com", "INBOX", &latest).unwrap();
        storage.store("me@example.com", "Sent", &reply).unwrap();
        assert_eq!(
            storage
                .related_sent("me@example.com", "INBOX", std::slice::from_ref(&latest))
                .unwrap()
                .len(),
            1
        );
        let history = [
            message(1, "original", "me@example.com", &[]),
            message(2, "question", "salon@example.com", &["original"]),
            message(3, "answer", "me@example.com", &["question"]),
            message(4, "offer", "salon@example.com", &["answer"]),
            message(5, "accepted", "me@example.com", &["offer"]),
            message(6, "confirmed", "salon@example.com", &[]),
            message(7, "unrelated", "another@example.com", &[]),
            Message {
                recipients: "another@example.com".into(),
                ..message(8, "other-recipient", "me@example.com", &[])
            },
        ];
        assert!(
            storage
                .store_conversation_headers("me@example.com", "All Mail", None, 7, &history)
                .unwrap()
        );
        storage
            .store(
                "other@example.com",
                "Archive",
                &message(1, "wrong-account", "salon@example.com", &[]),
            )
            .unwrap();
        let related = storage
            .related_sent("me@example.com", "INBOX", &[latest])
            .unwrap();
        let ids: HashSet<_> = related
            .iter()
            .map(|item| item.message.message_id.as_str())
            .collect();
        assert_eq!(
            ids,
            HashSet::from(["original", "question", "answer", "offer", "accepted"])
        );
        assert_eq!(related.len(), ids.len());
        assert!(
            storage
                .missing_conversation_uids("me@example.com", "All Mail", 7, &[1, 2, 3, 20])
                .unwrap()
                == [20]
        );
        assert_eq!(
            storage
                .missing_conversation_uids("me@example.com", "All Mail", 8, &[1, 2])
                .unwrap(),
            [1, 2]
        );
    }

    #[test]
    fn follows_changed_subjects_and_older_messages_in_the_same_folder() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let latest = message(4, "last", "salon@example.com", &["middle"]);
        let middle = Message {
            subject: "An updated subject".into(),
            ..message(3, "middle", "salon@example.com", &["first"])
        };
        let first = Message {
            subject: "Original topic".into(),
            ..message(2, "first", "me@example.com", &[])
        };
        storage.store("me@example.com", "INBOX", &latest).unwrap();
        storage.store("me@example.com", "Archive", &middle).unwrap();
        storage.store("me@example.com", "INBOX", &first).unwrap();
        let related = storage
            .related_sent("me@example.com", "INBOX", &[latest])
            .unwrap();
        assert_eq!(
            related
                .iter()
                .map(|item| item.message.message_id.as_str())
                .collect::<Vec<_>>(),
            ["middle", "first"]
        );
    }

    #[test]
    fn maintains_and_backfills_index_and_uses_it_for_lookups() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let mut header = message(1, "one", "salon@example.com", &[]);
        storage.store("me@example.com", "Archive", &header).unwrap();
        let id = Mailbox::find(&storage.0, "me@example.com", "Archive")
            .unwrap()
            .unwrap()
            .id(1);
        header.subject = "Changed".into();
        header.references = vec!["ancestor".into()];
        storage
            .write(|transaction| messages::refresh(transaction, id, &header))
            .unwrap();
        let stored_keys = || {
            storage
                .0
                .prepare("SELECT kind, value FROM rust_conversation_keys WHERE header_id=?1")
                .unwrap()
                .query_map([id], |row| {
                    Ok((row.get::<_, u8>(0)?, row.get::<_, String>(1)?))
                })
                .unwrap()
                .collect::<rusqlite::Result<HashSet<_>>>()
                .unwrap()
        };
        assert_eq!(stored_keys(), keys(&header).into_iter().collect());
        storage
            .write(|transaction| {
                transaction.execute("DELETE FROM rust_conversation_keys", [])?;
                transaction.execute(
                    "DELETE FROM rust_metadata WHERE key='conversation_keys'",
                    [],
                )?;
                migrate(transaction)
            })
            .unwrap();
        assert_eq!(stored_keys(), keys(&header).into_iter().collect());
        let plan: String = storage.0.query_row("EXPLAIN QUERY PLAN SELECT header_id FROM rust_conversation_keys WHERE account_id=?1 AND kind=?2 AND value=?3", params!["me@example.com", 0, "changed"], |row| row.get(3)).unwrap();
        assert!(
            plan.contains("SEARCH") && plan.contains("COVERING INDEX rust_conversation_lookup"),
            "{plan}"
        );
        storage
            .0
            .execute("DELETE FROM rust_headers WHERE id=?1", [id])
            .unwrap();
        assert!(stored_keys().is_empty());
    }

    #[test]
    fn partial_server_results_preserve_bodies_and_flags_and_reject_stale_mailboxes() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let original = Message {
            is_read: true,
            is_flagged: true,
            body_loaded: true,
            body_text: "Keep the cached body".into(),
            ..message(1, "original", "salon@example.com", &[])
        };
        assert!(
            storage
                .store_conversation_headers(
                    "me@example.com",
                    "Archive",
                    None,
                    7,
                    std::slice::from_ref(&original)
                )
                .unwrap()
        );
        let header = Message {
            is_read: false,
            is_flagged: false,
            body_loaded: false,
            body_text: String::new(),
            ..original.clone()
        };
        assert!(
            storage
                .store_conversation_headers("me@example.com", "Archive", Some(7), 7, &[header])
                .unwrap()
        );
        let cached = storage
            .message("me@example.com", "Archive", 1)
            .unwrap()
            .unwrap();
        assert!(cached.is_read && cached.is_flagged && cached.body_loaded);
        assert_eq!(cached.body_text, original.body_text);
        storage
            .inventory("me@example.com", "Archive", 8, &[])
            .unwrap();
        assert!(
            !storage
                .store_conversation_headers(
                    "me@example.com",
                    "Archive",
                    Some(7),
                    7,
                    std::slice::from_ref(&original)
                )
                .unwrap()
        );
        assert!(
            storage
                .message("me@example.com", "Archive", 1)
                .unwrap()
                .is_none()
        );
        assert!(
            storage
                .store_conversation_headers(
                    "me@example.com",
                    "Archive",
                    Some(8),
                    8,
                    std::slice::from_ref(&original)
                )
                .unwrap()
        );
        storage
            .confirm_action(
                "me@example.com",
                "Archive",
                &[Message {
                    uid_validity: Some(8),
                    ..original.clone()
                }],
                crate::models::sender_action::SenderAction::Archive,
            )
            .unwrap();
        assert!(
            storage
                .store_conversation_headers("me@example.com", "Archive", Some(8), 8, &[original])
                .unwrap()
        );
        assert!(
            storage
                .message("me@example.com", "Archive", 1)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn large_cache_lookup_only_hydrates_the_first_page_of_the_matching_thread() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        storage
            .write(|transaction| {
                let mailbox = Mailbox::ensure(transaction, "me@example.com", "Archive")?;
                for uid in 1..=20_000 {
                    let header = Message {
                        subject: format!("Unrelated topic {uid}"),
                        ..message(uid, &format!("unrelated-{uid}"), "another@example.com", &[])
                    };
                    messages::add(
                        transaction,
                        mailbox.id(uid),
                        "me@example.com",
                        Some("Archive"),
                        &header,
                    )?;
                }
                for uid in 20_001..=20_100 {
                    let header = Message {
                        body_loaded: true,
                        body_text: "Cached body ".repeat(1000),
                        ..message(uid, &format!("thread-{uid}"), "salon@example.com", &[])
                    };
                    messages::add(
                        transaction,
                        mailbox.id(uid),
                        "me@example.com",
                        Some("Archive"),
                        &header,
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let latest = message(30_000, "latest", "salon@example.com", &[]);
        let started = std::time::Instant::now();
        let related = storage
            .related_sent("me@example.com", "INBOX", &[latest])
            .unwrap();
        eprintln!(
            "Conversation lookup among 20,100 headers: {:?}",
            started.elapsed()
        );
        storage
            .0
            .execute(
                "INSERT INTO rust_folders VALUES ('me@example.com','Archive',7)",
                [],
            )
            .unwrap();
        let started = std::time::Instant::now();
        let snapshots = storage
            .conversation_snapshots("me@example.com", related.iter().map(|item| &item.message))
            .unwrap();
        eprintln!(
            "Reconciliation snapshot among 20,100 headers: {:?}",
            started.elapsed()
        );
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].messages.len(), 100);
        assert!(
            snapshots[0]
                .messages
                .iter()
                .all(|message| message.body_text.is_empty())
        );
        assert_eq!(related.len(), 100);
        assert_eq!(
            related
                .iter()
                .filter(|item| item.message.body_loaded)
                .count(),
            25
        );
        assert!(
            related[25..]
                .iter()
                .all(|item| item.message.body_text.is_empty() && item.message.body_html.is_empty())
        );
    }
}
