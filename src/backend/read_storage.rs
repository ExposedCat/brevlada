use super::{Storage, messages::Mailbox};
use crate::models::Message;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::HashMap;

impl Storage {
    pub fn read_revision(&self) -> Result<i64> {
        Ok(self
            .0
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM rust_metadata WHERE key='read_revision'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub(super) fn next_read_revision(transaction: &Transaction<'_>) -> Result<i64> {
        Ok(transaction.query_row(
            "INSERT INTO rust_metadata (key,value) VALUES ('read_revision','1')
             ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1 RETURNING CAST(value AS INTEGER)",
            [], |row| row.get(0),
        )?)
    }

    pub(super) fn reads_since(
        connection: &Connection,
        mailbox: Mailbox,
        account: &str,
        folder: &str,
        validity: u32,
        revision: i64,
    ) -> Result<HashMap<u32, bool>> {
        let mut statement = connection.prepare(
            "SELECT changes.uid, messages.is_read
             FROM rust_read_changes AS changes JOIN rust_headers AS messages
             ON messages.id=?1 + changes.uid
             WHERE changes.account_id=?2 AND changes.folder=?3 AND changes.uid_validity=?4 AND changes.revision>?5",
        )?;
        Ok(statement
            .query_map(
                params![mailbox.first(), account, folder, validity, revision],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub(super) fn record_read(
        transaction: &Transaction<'_>,
        account: &str,
        folder: &str,
        validity: Option<u32>,
        uid: u32,
    ) -> Result<Option<Message>> {
        Self::record_read_state(transaction, account, folder, validity, uid, true)
    }

    pub(super) fn record_read_state(
        transaction: &Transaction<'_>,
        account: &str,
        folder: &str,
        validity: Option<u32>,
        uid: u32,
        read: bool,
    ) -> Result<Option<Message>> {
        let revision = Self::next_read_revision(transaction)?;
        let Some(mailbox) = Mailbox::find(transaction, account, folder)? else {
            return Ok(None);
        };
        let updated = transaction.execute(
            "UPDATE rust_headers SET is_read=?4, read_revision=?5 WHERE id=?3
             AND (SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2) IS ?6",
            params![account, folder, mailbox.id(uid), read, revision, validity],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        transaction.execute(
            "INSERT INTO rust_read_changes VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(account_id,folder,uid) DO UPDATE SET uid_validity=excluded.uid_validity, revision=excluded.revision",
            params![account, folder, uid, validity, revision],
        )?;
        Self::cached(transaction, account, folder, uid)
    }

    pub fn mark_read_cached(
        &mut self,
        account: &str,
        folder: &str,
        validity: Option<u32>,
        uid: u32,
    ) -> Result<Option<Message>> {
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let message = Self::record_read(&transaction, account, folder, validity, uid)?;
        transaction.commit()?;
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::mail_sync::Flags;
    use crate::models::sender_action::SenderAction;

    fn message() -> Message {
        Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            body_loaded: true,
            body_text: "Cached body".into(),
            ..Default::default()
        }
    }

    #[test]
    fn concurrent_folder_refresh_cannot_undo_a_confirmed_read() {
        let path = std::env::temp_dir().join(format!(
            "brevlada-read-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut refresh = Storage::open(&path).unwrap();
        let mut reader = Storage::open(&path).unwrap();
        refresh
            .reconcile("a", "INBOX", 7, vec![message()], &[1])
            .unwrap();
        let started = refresh.read_revision().unwrap();
        let confirmed = reader
            .mark_read_cached("a", "INBOX", Some(7), 1)
            .unwrap()
            .unwrap();
        let flags = [Flags {
            uid: 1,
            read: false,
            flagged: true,
            draft: false,
        }];
        refresh
            .inventory_since("a", "INBOX", 7, &flags, started)
            .unwrap();
        let current = refresh.message("a", "INBOX", 1).unwrap().unwrap();
        assert!(current.is_read);
        assert!(current.is_flagged);
        assert_eq!(current.body_text, "Cached body");
        assert!(current.read_revision > confirmed.read_revision);
        assert!(
            !refresh
                .unread("a")
                .unwrap()
                .iter()
                .any(|(_, unread)| *unread)
        );
        let fresh = refresh.read_revision().unwrap();
        refresh
            .inventory_since("a", "INBOX", 7, &flags, fresh)
            .unwrap();
        let current = refresh.message("a", "INBOX", 1).unwrap().unwrap();
        assert!(!current.is_read);
        assert!(current.read_revision > confirmed.read_revision);
        refresh
            .inventory_since("a", "INBOX", 7, &flags, started)
            .unwrap();
        assert!(!refresh.message("a", "INBOX", 1).unwrap().unwrap().is_read);
        drop(reader);
        drop(refresh);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn stale_headers_keep_confirmed_read_and_newer_body_scoped_to_the_mailbox() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        for (account, folder) in [("a", "INBOX"), ("b", "INBOX"), ("a", "Archive")] {
            storage
                .reconcile(account, folder, 7, vec![message()], &[1])
                .unwrap();
        }
        let started = storage.read_revision().unwrap();
        storage
            .confirm_action("a", "INBOX", &[message()], SenderAction::MarkRead)
            .unwrap();
        let header = Message {
            body_loaded: false,
            body_text: String::new(),
            ..message()
        };
        for (account, folder) in [("a", "INBOX"), ("b", "INBOX"), ("a", "Archive")] {
            storage
                .reconcile_since(account, folder, 7, vec![header.clone()], &[1], started)
                .unwrap();
            let cached = storage.message(account, folder, 1).unwrap().unwrap();
            assert_eq!(cached.is_read, account == "a" && folder == "INBOX");
            assert_eq!(cached.body_text, "Cached body");
        }
        storage
            .reconcile_since("a", "INBOX", 8, vec![header], &[1], started)
            .unwrap();
        assert!(!storage.message("a", "INBOX", 1).unwrap().unwrap().is_read);
        assert!(
            storage
                .mark_read_cached("a", "INBOX", Some(7), 1)
                .unwrap()
                .is_none()
        );
    }
}
