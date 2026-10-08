use super::Storage;
use crate::{
    backend::{mail_sync::Flags, parser::normalize_message_id},
    models::Message,
};
use anyhow::{Result, ensure};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use std::collections::HashSet;

impl Storage {
    pub fn uncached_uids(&self, account: &str, folder: &str) -> Result<Vec<u32>> {
        let mut statement = self.0.prepare(
            "SELECT uid FROM rust_messages WHERE account_id=?1 AND folder=?2
             AND NOT coalesce(json_extract(data, '$.body_loaded'), 0)",
        )?;
        Ok(statement
            .query_map(params![account, folder], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    #[cfg(test)]
    pub fn messages(&self, account: &str, folder: &str) -> Result<Vec<Message>> {
        self.messages_with_limit(account, folder, crate::theme::MESSAGE_LIMIT)
    }

    #[cfg(test)]
    pub fn messages_with_limit(
        &self,
        account: &str,
        folder: &str,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT data FROM rust_messages WHERE account_id=?1 AND folder=?2
             ORDER BY json_extract(data, '$.timestamp') DESC, uid DESC LIMIT ?3",
        )?;
        let rows = statement.query_map(params![account, folder, limit as i64], |r| {
            r.get::<_, String>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn sender_page(
        &self,
        account: &str,
        folder: &str,
        sender: &str,
        before: Option<(i64, u32)>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT json_set(data, '$.inline_media', json('[]'), '$.inline_media_loaded', json('false'))
             FROM rust_messages WHERE account_id=?1 AND folder=?2
             AND instr(lower(json_extract(data, '$.sender')), ?3) > 0
             AND (?4 IS NULL OR (json_extract(data, '$.timestamp'), uid) < (?4, ?5))
             ORDER BY json_extract(data, '$.timestamp') DESC, uid DESC LIMIT ?6",
        )?;
        let mut cursor = before;
        let mut matched = Vec::new();
        let batch_size = limit.max(25);
        while matched.len() < limit {
            let rows = statement.query_map(
                params![
                    account,
                    folder,
                    sender,
                    cursor.map(|b| b.0),
                    cursor.map(|b| b.1),
                    batch_size as i64
                ],
                |row| row.get::<_, String>(0),
            )?;
            let batch: Vec<Message> = rows
                .map(|row| Ok(serde_json::from_str(&row?)?))
                .collect::<Result<_>>()?;
            let count = batch.len();
            cursor = batch.last().map(|message| (message.timestamp, message.uid));
            matched.extend(
                batch
                    .into_iter()
                    .filter(|message| crate::models::senders::key(message) == sender),
            );
            if count < batch_size {
                break;
            }
        }
        matched.truncate(limit);
        for message in &mut matched {
            message.prepare_display();
        }
        Ok(matched)
    }

    pub fn pending_bodies(
        &self,
        account: &str,
        folder: &str,
        before: Option<(i64, u32)>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT json_set(data, '$.body_text', '', '$.body_html', '', '$.attachments', json('[]'), '$.calendar_events', json('[]'), '$.tickets', json('[]'))
             FROM rust_messages WHERE account_id=?1 AND folder=?2
             AND NOT coalesce(json_extract(data, '$.body_loaded'), 0)
             AND (?3 IS NULL OR (json_extract(data, '$.timestamp'), uid) < (?3, ?4))
             ORDER BY json_extract(data, '$.timestamp') DESC, uid DESC LIMIT ?5"
        )?;
        let rows = statement.query_map(
            params![
                account,
                folder,
                before.map(|b| b.0),
                before.map(|b| b.1),
                limit as i64
            ],
            |r| r.get::<_, String>(0),
        )?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn validity(&self, account: &str, folder: &str) -> Result<Option<u32>> {
        Ok(self
            .0
            .query_row(
                "SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    #[cfg(test)]
    pub fn inventory(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        flags: &[Flags],
    ) -> Result<Vec<u32>> {
        self.inventory_since(account, folder, validity, flags, self.read_revision()?)
    }

    pub fn inventory_since(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        flags: &[Flags],
        snapshot_revision: i64,
    ) -> Result<Vec<u32>> {
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision = Self::next_read_revision(&transaction)?;
        let reads = Self::reads_since(&transaction, account, folder, validity, snapshot_revision)?;
        let starred =
            Self::flags_since(&transaction, account, folder, validity, snapshot_revision)?;
        let previous: Option<u32> = transaction
            .query_row(
                "SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Self::observe_notifications(
            &transaction,
            account,
            folder,
            validity,
            previous,
            &flags
                .iter()
                .map(|item| {
                    (
                        item.uid,
                        !reads.get(&item.uid).copied().unwrap_or(item.read) && !item.draft,
                    )
                })
                .collect::<Vec<_>>(),
        )?;
        if previous != Some(validity) {
            transaction.execute(
                "DELETE FROM rust_messages WHERE account_id=?1 AND folder=?2",
                params![account, folder],
            )?;
        }
        let existing: HashSet<u32> = {
            let mut statement = transaction
                .prepare("SELECT uid FROM rust_messages WHERE account_id=?1 AND folder=?2")?;
            statement
                .query_map(params![account, folder], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let removed = Self::removed_in(&transaction, account, folder, validity)?;
        let live: HashSet<_> = flags
            .iter()
            .map(|item| item.uid)
            .filter(|uid| !removed.contains(uid))
            .collect();
        for uid in existing.difference(&live) {
            transaction.execute(
                "DELETE FROM rust_messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
                params![account, folder, uid],
            )?;
        }
        for item in flags.iter().filter(|item| existing.contains(&item.uid)) {
            transaction.execute(
                "UPDATE rust_messages SET data=json_set(data, '$.is_read', json(?4), '$.is_flagged', json(?5), '$.is_draft', json(?6), '$.read_revision', ?7, '$.flag_revision', ?7)
                 WHERE account_id=?1 AND folder=?2 AND uid=?3",
                params![account, folder, item.uid, if reads.get(&item.uid).copied().unwrap_or(item.read) { "true" } else { "false" }, if starred.get(&item.uid).copied().unwrap_or(item.flagged) { "true" } else { "false" }, if item.draft { "true" } else { "false" }, revision]
            )?;
        }
        transaction.execute(
            "INSERT OR REPLACE INTO rust_folders VALUES (?1,?2,?3)",
            params![account, folder, validity],
        )?;
        transaction.execute(
            "INSERT OR REPLACE INTO rust_unread VALUES (?1,?2,?3)",
            params![
                account,
                folder,
                flags
                    .iter()
                    .any(|f| !reads.get(&f.uid).copied().unwrap_or(f.read)
                        && !removed.contains(&f.uid))
            ],
        )?;
        transaction.commit()?;
        let mut missing: Vec<_> = live.difference(&existing).copied().collect();
        missing.sort_unstable_by(|a, b| b.cmp(a));
        Ok(missing)
    }

    pub fn store_headers(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        messages: &[Message],
    ) -> Result<()> {
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let revision = Self::next_read_revision(&transaction)?;
        let current: Option<u32> = transaction
            .query_row(
                "SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        ensure!(current == Some(validity), "Mailbox changed during sync");
        let removed = Self::removed_in(&transaction, account, folder, validity)?;
        for message in messages
            .iter()
            .filter(|message| !removed.contains(&message.uid))
        {
            let mut message = message.clone();
            message.read_revision = revision;
            message.flag_revision = revision;
            transaction.execute(
                "INSERT OR IGNORE INTO rust_messages VALUES (?1,?2,?3,?4)",
                params![
                    account,
                    folder,
                    message.uid,
                    serde_json::to_string(&message)?
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn store_body(
        &mut self,
        account: &str,
        folder: &str,
        validity: Option<u32>,
        fetched: &Message,
    ) -> Result<Option<Message>> {
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<u32> = transaction
            .query_row(
                "SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if current != validity {
            return Ok(None);
        }
        let data: Option<String> = transaction
            .query_row(
                "SELECT data FROM rust_messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
                params![account, folder, fetched.uid],
                |r| r.get(0),
            )
            .optional()?;
        let Some(data) = data else {
            return Ok(None);
        };
        let mut message: Message = serde_json::from_str(&data)?;
        if !normalize_message_id(&message.message_id).is_empty()
            && !(message.is_draft && fetched.message_id.is_empty())
            && normalize_message_id(&message.message_id)
                != normalize_message_id(&fetched.message_id)
        {
            return Ok(None);
        }
        if message.is_draft && message.message_id.is_empty() {
            message.message_id = fetched.message_id.clone();
        }
        message.uid_validity = validity;
        message.reply_to = fetched.reply_to.clone();
        message.cc = fetched.cc.clone();
        message.in_reply_to = fetched.in_reply_to.clone();
        message.body_text = fetched.body_text.clone();
        message.body_html = fetched.body_html.clone();
        message.attachments = fetched.attachments.clone();
        message.attachment_details = fetched.attachment_details.clone();
        if fetched.inline_media_loaded {
            message.inline_media = fetched.inline_media.clone();
            message.inline_media_loaded = true;
        }
        message.unsubscribe = fetched.unsubscribe.clone().or(message.unsubscribe);
        message.parcels = fetched.parcels.clone();
        message.calendar_events = fetched.calendar_events.clone();
        message.tickets = fetched.tickets.clone();
        message.body_loaded = fetched.body_loaded;
        transaction.execute(
            "UPDATE rust_messages SET data=?4 WHERE account_id=?1 AND folder=?2 AND uid=?3",
            params![
                account,
                folder,
                message.uid,
                serde_json::to_string(&message)?
            ],
        )?;
        transaction.commit()?;
        Ok(Some(message))
    }

    pub fn store_unsubscribe(
        &self,
        account: &str,
        folder: &str,
        validity: Option<u32>,
        header: &Message,
    ) -> Result<()> {
        let _write = self.1.lock()?;
        let current = self
            .message(account, folder, header.uid)?
            .ok_or_else(|| anyhow::anyhow!("Message is no longer cached"))?;
        ensure!(
            normalize_message_id(&current.message_id) == normalize_message_id(&header.message_id),
            "The mailbox changed. Reload the folder."
        );
        let updated = self.0.execute(
            "UPDATE rust_messages SET data=json_set(data, '$.unsubscribe', json(?5))
             WHERE account_id=?1 AND folder=?2 AND uid=?3
             AND (SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2) IS ?4
             AND json_extract(data, '$.message_id')=?6",
            params![
                account,
                folder,
                header.uid,
                validity,
                serde_json::to_string(&header.unsubscribe)?,
                current.message_id
            ],
        )?;
        ensure!(
            updated == 1,
            "Mailbox changed while loading unsubscribe headers"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshes_legacy_unsubscribe_headers_without_overwriting_cached_body_or_flags() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let legacy = Message {
            uid: 2,
            message_id: "same".into(),
            body_loaded: true,
            body_text: "Cached body".into(),
            is_read: true,
            is_flagged: true,
            unsubscribe: Some(crate::models::Unsubscribe {
                url: "https://example.org/u".into(),
                one_click: None,
            }),
            ..Default::default()
        };
        storage.store("a", "INBOX", &legacy).unwrap();
        let header = Message {
            unsubscribe: Some(crate::models::Unsubscribe {
                url: "https://example.org/u".into(),
                one_click: Some(true),
            }),
            body_loaded: false,
            body_text: String::new(),
            is_read: false,
            is_flagged: false,
            ..legacy.clone()
        };
        storage
            .store_unsubscribe("a", "INBOX", None, &header)
            .unwrap();
        assert_eq!(
            storage.message("a", "INBOX", 2).unwrap().unwrap(),
            Message {
                unsubscribe: header.unsubscribe.clone(),
                ..legacy
            }
        );
        assert!(
            storage
                .store_unsubscribe("a", "INBOX", Some(99), &header)
                .is_err()
        );
        assert!(
            storage
                .store_unsubscribe(
                    "a",
                    "INBOX",
                    None,
                    &Message {
                        message_id: "replaced".into(),
                        ..header
                    }
                )
                .is_err()
        );
        assert_eq!(
            storage
                .message("a", "INBOX", 2)
                .unwrap()
                .unwrap()
                .unsubscribe
                .unwrap()
                .one_click,
            Some(true)
        );
    }

    #[test]
    fn resumes_missing_work_and_rejects_obsolete_bodies() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let flags = [
            Flags {
                uid: 1,
                read: false,
                flagged: false,
                draft: false,
            },
            Flags {
                uid: 2,
                read: false,
                flagged: false,
                draft: false,
            },
        ];
        assert_eq!(
            storage.inventory("a", "INBOX", 7, &flags).unwrap(),
            vec![2, 1]
        );
        let header = Message {
            uid: 2,
            message_id: "new".into(),
            ..Default::default()
        };
        storage
            .store_headers("a", "INBOX", 7, std::slice::from_ref(&header))
            .unwrap();
        assert_eq!(storage.inventory("a", "INBOX", 7, &flags).unwrap(), vec![1]);
        let fetched = Message {
            body_loaded: true,
            body_text: "Cached body".into(),
            tickets: vec![crate::models::ticket::Reservation {
                name: "Express".into(),
                kind: crate::models::ticket::Kind::Train,
                number: Some("booking-one".into()),
                ..Default::default()
            }],
            ..header.clone()
        };
        let read = Message {
            is_read: true,
            is_flagged: true,
            ..header.clone()
        };
        storage.store("a", "INBOX", &read).unwrap();
        let stored = storage
            .store_body("a", "INBOX", Some(7), &fetched)
            .unwrap()
            .unwrap();
        assert!(stored.is_read && stored.is_flagged && stored.body_loaded);
        assert_eq!(stored.tickets, fetched.tickets);
        let marked = storage
            .mark_read_cached("a", "INBOX", Some(7), 2)
            .unwrap()
            .unwrap();
        assert!(marked.is_read && marked.is_flagged && marked.body_loaded);
        assert!(
            storage
                .mark_read_cached("a", "INBOX", Some(8), 2)
                .unwrap()
                .is_none()
        );
        assert!(
            storage
                .pending_bodies("a", "INBOX", None, 128)
                .unwrap()
                .is_empty()
        );
        storage.inventory("a", "INBOX", 8, &flags).unwrap();
        assert!(
            storage
                .store_body("a", "INBOX", Some(7), &fetched)
                .unwrap()
                .is_none()
        );
        storage.store_headers("a", "INBOX", 8, &[header]).unwrap();
        storage.inventory("a", "INBOX", 8, &[]).unwrap();
        assert!(
            storage
                .store_body("a", "INBOX", Some(8), &fetched)
                .unwrap()
                .is_none()
        );
        assert!(storage.messages("a", "INBOX").unwrap().is_empty());
    }
}
