use super::Storage;
use crate::models::{Message, sender_action::SenderAction};
use anyhow::{Context, Result};
use rusqlite::{TransactionBehavior, params};

impl Storage {
    pub fn confirm_action(
        &mut self,
        account: &str,
        folder: &str,
        messages: &[Message],
        action: SenderAction,
    ) -> Result<()> {
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for message in messages {
            let validity = message
                .uid_validity
                .context("Missing message UIDVALIDITY")?;
            if action == SenderAction::MarkRead {
                Self::record_read(&transaction, account, folder, Some(validity), message.uid)?;
            } else {
                transaction.execute(
                    "INSERT OR IGNORE INTO rust_removed_messages VALUES (?1,?2,?3,?4)",
                    params![account, folder, validity, message.uid],
                )?;
                transaction.execute(
                    "DELETE FROM rust_messages WHERE account_id=?1 AND folder=?2 AND uid=?3
                     AND (SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2)=?4",
                    params![account, folder, message.uid, validity],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn removed_in(
        connection: &rusqlite::Connection,
        account: &str,
        folder: &str,
        validity: u32,
    ) -> Result<std::collections::HashSet<u32>> {
        let mut statement = connection.prepare(
            "SELECT uid FROM rust_removed_messages WHERE account_id=?1 AND folder=?2 AND uid_validity=?3",
        )?;
        Ok(statement
            .query_map(params![account, folder, validity], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmed_removals_survive_stale_sync_writes_and_uid_reuse_is_scoped() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let message = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            ..Default::default()
        };
        storage
            .reconcile("a", "INBOX", 7, vec![message.clone()], &[1])
            .unwrap();
        storage
            .confirm_action(
                "a",
                "INBOX",
                std::slice::from_ref(&message),
                SenderAction::Archive,
            )
            .unwrap();
        storage
            .store_headers("a", "INBOX", 7, std::slice::from_ref(&message))
            .unwrap();
        storage
            .reconcile("a", "INBOX", 7, vec![message.clone()], &[1])
            .unwrap();
        let flags = [crate::backend::mail_sync::Flags {
            uid: 1,
            read: false,
            flagged: false,
            draft: false,
        }];
        assert!(
            storage
                .inventory("a", "INBOX", 7, &flags)
                .unwrap()
                .is_empty()
        );
        assert!(storage.message("a", "INBOX", 1).unwrap().is_none());
        assert!(
            storage
                .store_body("a", "INBOX", Some(7), &message)
                .unwrap()
                .is_none()
        );
        storage
            .reconcile("b", "INBOX", 7, vec![message.clone()], &[1])
            .unwrap();
        storage
            .reconcile("a", "Archive", 7, vec![message.clone()], &[1])
            .unwrap();
        assert!(storage.message("b", "INBOX", 1).unwrap().is_some());
        assert!(storage.message("a", "Archive", 1).unwrap().is_some());
        storage
            .reconcile("a", "INBOX", 8, vec![message], &[1])
            .unwrap();
        assert_eq!(
            storage
                .message("a", "INBOX", 1)
                .unwrap()
                .unwrap()
                .uid_validity,
            Some(8)
        );
    }
}
