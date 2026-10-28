use super::{Storage, messages::Mailbox};
use crate::{backend::parser::normalize_message_id, models::Message};
use anyhow::Result;
use rusqlite::params;
use std::collections::{BTreeMap, HashSet};

const SNAPSHOT_QUERY: &str =
    "SELECT h.uid, h.uid_validity, h.message_id, h.read_revision, h.flag_revision,
                    h.folder, f.uid_validity
             FROM json_each(?2) wanted CROSS JOIN rust_conversation_keys k CROSS JOIN rust_headers h
             JOIN rust_folders f ON f.account_id=h.account_id AND f.folder=h.folder
             WHERE k.account_id=?1 AND k.kind=1 AND k.value=wanted.value AND h.id=k.header_id
             AND h.account_id=?1 AND f.uid_validity IS NOT NULL
             AND trim(h.message_id, '<> ' || char(9) || char(10) || char(13))=wanted.value";

pub(in crate::backend) struct Snapshot {
    pub account: String,
    pub folder: String,
    pub validity: u32,
    pub messages: Vec<Message>,
}

impl Storage {
    pub(in crate::backend) fn conversation_snapshots<'a>(
        &self,
        account: &str,
        messages: impl IntoIterator<Item = &'a Message>,
    ) -> Result<Vec<Snapshot>> {
        let ids: HashSet<_> = messages
            .into_iter()
            .map(|message| normalize_message_id(&message.message_id))
            .filter(|id| !id.is_empty())
            .collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self.0.prepare_cached(SNAPSHOT_QUERY)?;
        let rows = statement.query_map(params![account, serde_json::to_string(&ids)?], |row| {
            Ok((
                row.get::<_, String>(5)?,
                row.get::<_, u32>(6)?,
                Message {
                    uid: row.get(0)?,
                    uid_validity: row.get(1)?,
                    message_id: row.get(2)?,
                    read_revision: row.get(3)?,
                    flag_revision: row.get(4)?,
                    ..Default::default()
                },
            ))
        })?;
        let mut folders: BTreeMap<(String, u32), Vec<Message>> = BTreeMap::new();
        for row in rows {
            let (folder, validity, message) = row?;
            if message.uid_validity.is_none_or(|cached| cached == validity) {
                folders.entry((folder, validity)).or_default().push(message);
            }
        }
        Ok(folders
            .into_iter()
            .map(|((folder, validity), messages)| Snapshot {
                account: account.to_owned(),
                folder,
                validity,
                messages,
            })
            .collect())
    }

    pub(in crate::backend) fn reconcile_conversation(
        &self,
        snapshot: &Snapshot,
        present: &[u32],
    ) -> Result<Vec<Message>> {
        let present: HashSet<_> = present.iter().copied().collect();
        self.write(|transaction| {
            if Self::folder_validity(transaction, &snapshot.account, &snapshot.folder)? != Some(snapshot.validity) {
                return Ok(Vec::new());
            }
            let Some(mailbox) = Mailbox::find(transaction, &snapshot.account, &snapshot.folder)? else { return Ok(Vec::new()); };
            let mut removed = Vec::new();
            for message in snapshot.messages.iter().filter(|message| !present.contains(&message.uid)) {
                let deleted = transaction.prepare_cached(
                    "DELETE FROM rust_headers WHERE id=?1 AND message_id=?2 AND uid_validity IS ?3
                     AND read_revision=?4 AND flag_revision=?5"
                )?.execute(params![mailbox.id(message.uid), message.message_id, message.uid_validity, message.read_revision, message.flag_revision])?;
                if deleted == 0 { continue; }
                transaction.prepare_cached("INSERT OR IGNORE INTO rust_removed_messages VALUES (?1,?2,?3,?4)")?
                    .execute(params![snapshot.account, snapshot.folder, snapshot.validity, message.uid])?;
                let mut message = message.clone();
                message.uid_validity = Some(snapshot.validity);
                removed.push(message);
            }
            Ok(removed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::sender_action::SenderAction;

    fn message(uid: u32, id: &str) -> Message {
        Message {
            uid,
            uid_validity: Some(7),
            message_id: id.into(),
            subject: "Appointment".into(),
            sender: "me@example.com".into(),
            recipients: "salon@example.com".into(),
            body_loaded: true,
            body_text: "Hi, that works for us.".into(),
            is_read: true,
            ..Default::default()
        }
    }

    #[test]
    fn removes_only_confirmed_missing_copies_and_keeps_late_sync_from_restoring_them() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let stale = message(7418, "old-draft");
        let sent = message(7419, "sent");
        let unrelated = message(8000, "other-thread");
        storage
            .store_conversation_headers(
                "me",
                "All Mail",
                None,
                7,
                &[stale.clone(), sent.clone(), unrelated.clone()],
            )
            .unwrap();
        storage
            .store_conversation_headers(
                "me",
                "Sent",
                None,
                7,
                &[Message {
                    uid: 344,
                    ..sent.clone()
                }],
            )
            .unwrap();
        storage
            .store_conversation_headers("other", "All Mail", None, 7, std::slice::from_ref(&stale))
            .unwrap();
        storage
            .store_sent(
                "me",
                &Message {
                    uid: 0,
                    ..sent.clone()
                },
            )
            .unwrap();
        let snapshots = storage
            .conversation_snapshots("me", [&stale, &sent])
            .unwrap();
        assert_eq!(snapshots.len(), 2);
        let snapshot = snapshots
            .iter()
            .find(|snapshot| snapshot.folder == "All Mail")
            .unwrap();
        assert_eq!(snapshot.messages.len(), 2);
        assert!(
            snapshot
                .messages
                .iter()
                .all(|message| message.body_text.is_empty() && message.body_html.is_empty())
        );
        let removed = storage
            .reconcile_conversation(snapshot, &[7419, 9999])
            .unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].message_id, "old-draft");
        assert!(storage.message("me", "All Mail", 7418).unwrap().is_none());
        assert!(
            storage
                .message("me", "All Mail", 7419)
                .unwrap()
                .unwrap()
                .body_loaded
        );
        assert!(storage.message("me", "All Mail", 8000).unwrap().is_some());
        assert!(storage.message("me", "Sent", 344).unwrap().is_some());
        assert!(
            storage
                .message("other", "All Mail", 7418)
                .unwrap()
                .is_some()
        );
        let id = Mailbox::find(&storage.0, "me", "All Mail")
            .unwrap()
            .unwrap()
            .id(7418);
        for table in ["rust_headers", "rust_bodies"] {
            assert_eq!(
                storage
                    .0
                    .query_row(
                        &format!("SELECT count(*) FROM {table} WHERE id=?1"),
                        [id],
                        |row| row.get::<_, u32>(0)
                    )
                    .unwrap(),
                0
            );
        }
        assert_eq!(
            storage
                .0
                .query_row(
                    "SELECT count(*) FROM rust_conversation_keys WHERE header_id=?1",
                    [id],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
            0
        );
        assert_eq!(
            storage
                .0
                .query_row(
                    "SELECT count(*) FROM rust_headers WHERE account_id='me' AND folder IS NULL",
                    [],
                    |row| row.get::<_, u32>(0)
                )
                .unwrap(),
            1
        );
        storage
            .store_headers("me", "All Mail", 7, std::slice::from_ref(&stale))
            .unwrap();
        storage
            .store_conversation_headers("me", "All Mail", Some(7), 7, std::slice::from_ref(&stale))
            .unwrap();
        storage
            .reconcile("me", "All Mail", 7, vec![stale], &[7418, 7419, 8000])
            .unwrap();
        assert!(storage.message("me", "All Mail", 7418).unwrap().is_none());
    }

    #[test]
    fn protects_read_and_star_changes_replaced_headers_and_reused_mailbox_uids() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let original = message(1, "original");
        storage
            .store_conversation_headers("me", "Archive", None, 7, std::slice::from_ref(&original))
            .unwrap();
        for action in [SenderAction::MarkUnread, SenderAction::Star] {
            let snapshot = storage
                .conversation_snapshots("me", [&original])
                .unwrap()
                .remove(0);
            storage
                .confirm_action("me", "Archive", std::slice::from_ref(&original), action)
                .unwrap();
            assert!(
                storage
                    .reconcile_conversation(&snapshot, &[])
                    .unwrap()
                    .is_empty()
            );
            assert!(storage.message("me", "Archive", 1).unwrap().is_some());
        }
        let snapshot = storage
            .conversation_snapshots("me", [&original])
            .unwrap()
            .remove(0);
        let replacement = message(1, "replacement");
        storage.store("me", "Archive", &replacement).unwrap();
        assert!(
            storage
                .reconcile_conversation(&snapshot, &[])
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            storage
                .message("me", "Archive", 1)
                .unwrap()
                .unwrap()
                .message_id,
            "replacement"
        );
        storage.inventory("me", "Archive", 8, &[]).unwrap();
        storage
            .store_conversation_headers(
                "me",
                "Archive",
                Some(8),
                8,
                std::slice::from_ref(&original),
            )
            .unwrap();
        assert!(
            storage
                .reconcile_conversation(&snapshot, &[])
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            storage
                .message("me", "Archive", 1)
                .unwrap()
                .unwrap()
                .uid_validity,
            Some(8)
        );
        assert!(
            !Storage::removed_in(&storage.0, "me", "Archive", 7)
                .unwrap()
                .contains(&1)
        );
    }

    #[test]
    fn reconciles_legacy_headers_without_explicit_uid_validity_after_folder_validation() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let stale = message(1, "legacy");
        storage
            .store_conversation_headers("me", "Archive", None, 7, std::slice::from_ref(&stale))
            .unwrap();
        storage
            .0
            .execute("UPDATE rust_headers SET uid_validity=NULL", [])
            .unwrap();
        let snapshot = storage
            .conversation_snapshots("me", [&stale])
            .unwrap()
            .remove(0);
        assert_eq!(snapshot.validity, 7);
        assert_eq!(snapshot.messages[0].uid_validity, None);
        let removed = storage.reconcile_conversation(&snapshot, &[]).unwrap();
        assert_eq!(removed[0].uid_validity, Some(7));
        assert!(storage.message("me", "Archive", 1).unwrap().is_none());
    }

    #[test]
    fn snapshot_uses_the_full_conversation_key_and_primary_key_instead_of_scanning_an_account() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let plan = storage
            .0
            .prepare(&format!("EXPLAIN QUERY PLAN {SNAPSHOT_QUERY}"))
            .unwrap()
            .query_map(params!["me", "[\"one\"]"], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        assert!(plan.contains("SEARCH k USING COVERING INDEX rust_conversation_lookup (account_id=? AND kind=? AND value=?)"), "{plan}");
        assert!(
            plan.contains("SEARCH h USING INTEGER PRIMARY KEY"),
            "{plan}"
        );
    }
}
