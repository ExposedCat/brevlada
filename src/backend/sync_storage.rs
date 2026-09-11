use super::Storage;
use crate::{
    backend::{mail_sync::Flags, parser::normalize_message_id},
    models::Message,
    theme,
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

    pub fn messages(&self, account: &str, folder: &str) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT data FROM rust_messages WHERE account_id=?1 AND folder=?2
             ORDER BY json_extract(data, '$.timestamp') DESC, uid DESC LIMIT ?3",
        )?;
        let rows = statement
            .query_map(params![account, folder, theme::MESSAGE_LIMIT as i64], |r| {
                r.get::<_, String>(0)
            })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }

    pub fn pending_bodies(
        &self,
        account: &str,
        folder: &str,
        before: Option<(i64, u32)>,
        limit: usize,
    ) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT json_set(data, '$.body_text', '', '$.body_html', '', '$.attachments', json('[]'))
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

    pub fn mark_read_cached(
        &self,
        account: &str,
        folder: &str,
        validity: Option<u32>,
        uid: u32,
    ) -> Result<Option<Message>> {
        let data: Option<String> = self
            .0
            .query_row(
                "UPDATE rust_messages SET data=json_set(data, '$.is_read', json('true'))
             WHERE account_id=?1 AND folder=?2 AND uid=?3
             AND (SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2) IS ?4
             RETURNING data",
                params![account, folder, uid, validity],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|data| Ok(serde_json::from_str(&data)?))
            .transpose()
    }

    pub fn inventory(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        flags: &[Flags],
    ) -> Result<Vec<u32>> {
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<u32> = transaction
            .query_row(
                "SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
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
        let live: HashSet<_> = flags.iter().map(|item| item.uid).collect();
        for uid in existing.difference(&live) {
            transaction.execute(
                "DELETE FROM rust_messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
                params![account, folder, uid],
            )?;
        }
        for item in flags.iter().filter(|item| existing.contains(&item.uid)) {
            transaction.execute(
                "UPDATE rust_messages SET data=json_set(data, '$.is_read', json(?4), '$.is_flagged', json(?5))
                 WHERE account_id=?1 AND folder=?2 AND uid=?3
                 AND (json_extract(data, '$.is_read') != json_extract(?4, '$')
                      OR coalesce(json_extract(data, '$.is_flagged'), 0) != json_extract(?5, '$'))",
                params![account, folder, item.uid, if item.read { "true" } else { "false" }, if item.flagged { "true" } else { "false" }]
            )?;
        }
        transaction.execute(
            "INSERT OR REPLACE INTO rust_folders VALUES (?1,?2,?3)",
            params![account, folder, validity],
        )?;
        transaction.execute(
            "INSERT OR REPLACE INTO rust_unread VALUES (?1,?2,?3)",
            params![account, folder, flags.iter().any(|f| !f.read)],
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
        ensure!(current == Some(validity), "Mailbox changed during sync");
        for message in messages {
            transaction.execute(
                "INSERT OR IGNORE INTO rust_messages VALUES (?1,?2,?3,?4)",
                params![
                    account,
                    folder,
                    message.uid,
                    serde_json::to_string(message)?
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
            && normalize_message_id(&message.message_id)
                != normalize_message_id(&fetched.message_id)
        {
            return Ok(None);
        }
        message.body_text = fetched.body_text.clone();
        message.body_html = fetched.body_html.clone();
        message.attachments = fetched.attachments.clone();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumes_missing_work_and_rejects_obsolete_bodies() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let flags = [
            Flags {
                uid: 1,
                read: false,
                flagged: false,
            },
            Flags {
                uid: 2,
                read: false,
                flagged: false,
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
