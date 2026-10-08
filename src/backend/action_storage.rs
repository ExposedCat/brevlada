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
            if let Some(read) = action.read_state() {
                Self::record_read_state(
                    &transaction,
                    account,
                    folder,
                    Some(validity),
                    message.uid,
                    read,
                )?;
            } else if let Some(flagged) = action.flagged_state() {
                let revision = Self::next_read_revision(&transaction)?;
                transaction.execute(
                    "UPDATE rust_messages SET data=json_set(data, '$.is_flagged', json(?5), '$.flag_revision', ?6)
                     WHERE account_id=?1 AND folder=?2 AND uid=?3
                     AND (SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2)=?4",
                    params![account, folder, message.uid, validity, if flagged { "true" } else { "false" }, revision],
                )?;
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

    pub(super) fn flags_since(
        connection: &rusqlite::Connection,
        account: &str,
        folder: &str,
        validity: u32,
        revision: i64,
    ) -> Result<std::collections::HashMap<u32, bool>> {
        let mut statement = connection.prepare(
            "SELECT uid, json_extract(data, '$.is_flagged') FROM rust_messages
             WHERE account_id=?1 AND folder=?2
             AND (SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2)=?3
             AND coalesce(json_extract(data, '$.flag_revision'), 0)>?4",
        )?;
        Ok(statement
            .query_map(params![account, folder, validity, revision], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?)
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
    fn unread_and_star_changes_survive_stale_inventory_headers_and_bodies() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let message = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            is_read: true,
            body_loaded: true,
            body_text: "Cached body".into(),
            ..Default::default()
        };
        for folder in ["INBOX", "Archive"] {
            storage
                .reconcile("a", folder, 7, vec![message.clone()], &[1])
                .unwrap();
        }
        let started = storage.read_revision().unwrap();
        for action in [SenderAction::MarkUnread, SenderAction::Star] {
            storage
                .confirm_action("a", "INBOX", std::slice::from_ref(&message), action)
                .unwrap();
        }
        let flags = [crate::backend::mail_sync::Flags {
            uid: 1,
            read: true,
            flagged: false,
            draft: false,
        }];
        storage
            .inventory_since("a", "INBOX", 7, &flags, started)
            .unwrap();
        storage
            .reconcile_since("a", "INBOX", 7, vec![message.clone()], &[1], started)
            .unwrap();
        let current = storage
            .store_body("a", "INBOX", Some(7), &message)
            .unwrap()
            .unwrap();
        assert!(!current.is_read && current.is_flagged);
        assert!(current.read_revision > started && current.flag_revision > started);
        assert_eq!(current.body_text, "Cached body");
        let other = storage.message("a", "Archive", 1).unwrap().unwrap();
        assert!(other.is_read && !other.is_flagged);
        assert!(
            storage
                .unread("a")
                .unwrap()
                .contains(&("INBOX".into(), true))
        );

        let before_unstar = storage.read_revision().unwrap();
        storage
            .confirm_action("a", "INBOX", &[message], SenderAction::Unstar)
            .unwrap();
        let stale = [crate::backend::mail_sync::Flags {
            flagged: true,
            ..flags[0]
        }];
        storage
            .inventory_since("a", "INBOX", 7, &stale, before_unstar)
            .unwrap();
        assert!(
            !storage
                .message("a", "INBOX", 1)
                .unwrap()
                .unwrap()
                .is_flagged
        );
        storage.inventory("a", "INBOX", 8, &stale).unwrap();
        assert!(storage.message("a", "INBOX", 1).unwrap().is_none());
    }

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
