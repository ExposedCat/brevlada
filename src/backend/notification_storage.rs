use super::Storage;
use crate::models::Message;
use anyhow::Result;
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use std::collections::HashSet;

impl Storage {
    pub fn notification_folders(&self) -> Result<Vec<(String, String, u32)>> {
        let mut statement = self.0.prepare(
            "SELECT DISTINCT account_id,folder,uid_validity FROM rust_notification_pending",
        )?;
        Ok(statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub(super) fn observe_notifications(
        transaction: &Transaction<'_>,
        account: &str,
        folder: &str,
        validity: u32,
        previous: Option<u32>,
        flags: &[(u32, bool)],
    ) -> Result<()> {
        if !folder.eq_ignore_ascii_case("INBOX") {
            return Ok(());
        }
        let checkpoint: Option<(u32, u32)> = transaction.query_row(
            "SELECT uid_validity,last_uid FROM rust_notification_folders WHERE account_id=?1 AND folder=?2",
            params![account, folder], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?;
        let baseline = match checkpoint {
            Some((epoch, uid)) if epoch == validity => Some(uid),
            Some(_) => None,
            None if previous == Some(validity) => transaction.query_row(
                "SELECT max(uid) FROM rust_messages WHERE account_id=?1 AND folder=?2",
                params![account, folder],
                |row| row.get::<_, Option<u32>>(0),
            )?,
            None => None,
        };
        let unread: HashSet<_> = flags
            .iter()
            .filter(|(_, unread)| *unread)
            .map(|(uid, _)| *uid)
            .collect();
        let pending: Vec<(u32, u32)> = {
            let mut statement = transaction.prepare(
                "SELECT uid_validity,uid FROM rust_notification_pending WHERE account_id=?1 AND folder=?2",
            )?;
            statement
                .query_map(params![account, folder], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?
        };
        for (epoch, uid) in pending {
            if epoch != validity || !unread.contains(&uid) {
                transaction.execute(
                    "DELETE FROM rust_notification_pending WHERE account_id=?1 AND folder=?2 AND uid_validity=?3 AND uid=?4",
                    params![account, folder, epoch, uid],
                )?;
            }
        }
        if let Some(baseline) = baseline {
            for uid in unread.iter().filter(|uid| **uid > baseline) {
                transaction.execute(
                    "INSERT OR IGNORE INTO rust_notification_pending VALUES (?1,?2,?3,?4)",
                    params![account, folder, validity, uid],
                )?;
            }
        }
        let last_uid = flags
            .iter()
            .map(|(uid, _)| *uid)
            .max()
            .unwrap_or(0)
            .max(baseline.unwrap_or(0));
        transaction.execute(
            "INSERT OR REPLACE INTO rust_notification_folders VALUES (?1,?2,?3,?4)",
            params![account, folder, validity, last_uid],
        )?;
        Ok(())
    }

    pub fn pending_notifications(
        &self,
        account: &str,
        folder: &str,
        validity: u32,
    ) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT m.data FROM rust_notification_pending n
             JOIN rust_messages m ON m.account_id=n.account_id AND m.folder=n.folder AND m.uid=n.uid
             JOIN rust_folders f ON f.account_id=n.account_id AND f.folder=n.folder AND f.uid_validity=n.uid_validity
             WHERE n.account_id=?1 AND n.folder=?2 AND n.uid_validity=?3
             AND NOT coalesce(json_extract(m.data,'$.is_read'),0)
             AND NOT coalesce(json_extract(m.data,'$.is_draft'),0)
             ORDER BY n.uid DESC",
        )?;
        let rows = statement.query_map(params![account, folder, validity], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn acknowledge_notifications(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        uids: &[u32],
    ) -> Result<()> {
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for uid in uids {
            transaction.execute(
                "DELETE FROM rust_notification_pending WHERE account_id=?1 AND folder=?2 AND uid_validity=?3 AND uid=?4",
                params![account, folder, validity, uid],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::mail_sync::Flags;

    fn flags(items: &[(u32, bool)]) -> Vec<Flags> {
        items
            .iter()
            .map(|(uid, read)| Flags {
                uid: *uid,
                read: *read,
                flagged: false,
                draft: false,
            })
            .collect()
    }

    fn headers(storage: &mut Storage, validity: u32, uids: &[u32]) {
        let messages: Vec<_> = uids
            .iter()
            .map(|uid| Message {
                uid: *uid,
                uid_validity: Some(validity),
                message_id: format!("message-{validity}-{uid}"),
                subject: format!("Subject {uid}"),
                ..Message::default()
            })
            .collect();
        storage
            .store_headers("account", "INBOX", validity, &messages)
            .unwrap();
    }

    #[test]
    fn incomplete_historical_cache_does_not_notify_but_offline_arrivals_do() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        storage
            .inventory("account", "INBOX", 1, &flags(&[(1, false), (2, false)]))
            .unwrap();
        headers(&mut storage, 1, &[1]);
        assert!(
            storage
                .pending_notifications("account", "INBOX", 1)
                .unwrap()
                .is_empty()
        );
        let current = flags(&[(1, false), (2, false), (3, false), (4, true)]);
        storage.inventory("account", "INBOX", 1, &current).unwrap();
        headers(&mut storage, 1, &[2, 3]);
        assert_eq!(
            storage
                .pending_notifications("account", "INBOX", 1)
                .unwrap()
                .iter()
                .map(|message| message.uid)
                .collect::<Vec<_>>(),
            [3]
        );
        storage
            .acknowledge_notifications("account", "INBOX", 1, &[3])
            .unwrap();
        storage.inventory("account", "INBOX", 1, &current).unwrap();
        assert!(
            storage
                .pending_notifications("account", "INBOX", 1)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn upgrading_uses_existing_cache_as_baseline() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        storage
            .0
            .execute("INSERT INTO rust_folders VALUES ('account','INBOX',1)", [])
            .unwrap();
        headers(&mut storage, 1, &[10]);
        storage
            .inventory(
                "account",
                "INBOX",
                1,
                &flags(&[(9, false), (10, false), (11, false)]),
            )
            .unwrap();
        headers(&mut storage, 1, &[9, 11]);
        assert_eq!(
            storage
                .pending_notifications("account", "INBOX", 1)
                .unwrap()[0]
                .uid,
            11
        );
        assert_eq!(
            storage
                .pending_notifications("account", "INBOX", 1)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn pending_arrivals_survive_restart_and_uid_resets_are_silent() {
        let directory = std::env::temp_dir().join(format!(
            "brevlada-notifications-{}",
            gtk::glib::uuid_string_random()
        ));
        let path = directory.join("emails.db");
        let mut storage = Storage::open(&path).unwrap();
        storage
            .inventory("account", "INBOX", 1, &flags(&[(1, false)]))
            .unwrap();
        headers(&mut storage, 1, &[1]);
        storage
            .inventory("account", "INBOX", 1, &flags(&[(1, false), (2, false)]))
            .unwrap();
        drop(storage);
        let mut storage = Storage::open(&path).unwrap();
        storage
            .inventory("account", "INBOX", 1, &flags(&[(1, false), (2, false)]))
            .unwrap();
        headers(&mut storage, 1, &[2]);
        assert_eq!(
            storage
                .pending_notifications("account", "INBOX", 1)
                .unwrap()[0]
                .uid,
            2
        );
        storage
            .inventory("account", "INBOX", 2, &flags(&[(1, false), (2, false)]))
            .unwrap();
        headers(&mut storage, 2, &[1, 2]);
        assert!(
            storage
                .pending_notifications("account", "INBOX", 2)
                .unwrap()
                .is_empty()
        );
        assert!(storage.notification_folders().unwrap().is_empty());
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn read_and_removed_arrivals_are_discarded() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        storage.inventory("account", "INBOX", 1, &[]).unwrap();
        storage
            .inventory("account", "INBOX", 1, &flags(&[(1, false), (2, false)]))
            .unwrap();
        headers(&mut storage, 1, &[1, 2]);
        storage
            .inventory("account", "INBOX", 1, &flags(&[(1, true)]))
            .unwrap();
        assert!(
            storage
                .pending_notifications("account", "INBOX", 1)
                .unwrap()
                .is_empty()
        );
        assert!(storage.notification_folders().unwrap().is_empty());
    }
}
