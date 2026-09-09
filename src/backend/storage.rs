use crate::models::Message;
use anyhow::Result;
use rusqlite::{Connection, params};
use std::{collections::HashSet, path::Path};

pub struct Storage(Connection);

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::initialize(Connection::open(path)?)
    }

    fn initialize(mut connection: Connection) -> Result<Self> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS rust_messages (
            account_id TEXT NOT NULL, folder TEXT NOT NULL, uid INTEGER NOT NULL,
            data TEXT NOT NULL, PRIMARY KEY(account_id, folder, uid));
            CREATE TABLE IF NOT EXISTS rust_folders (
            account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER,
            PRIMARY KEY(account_id, folder));
            CREATE TABLE IF NOT EXISTS rust_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        let legacy: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='messages')",
            [],
            |r| r.get(0),
        )?;
        let migrated: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM rust_metadata WHERE key='legacy_import')",
            [],
            |r| r.get(0),
        )?;
        if legacy && !migrated {
            let mut statement = transaction.prepare("SELECT uid, account_id, folder, message_id, subject, sender_name, sender_email, recipients, date_sent, message_references, is_read, body_text, body_html, is_flagged FROM messages WHERE is_deleted=0")?;
            let rows = statement.query_map([], |row| {
                let value = |i| {
                    row.get::<_, Option<String>>(i)
                        .map(Option::unwrap_or_default)
                };
                let date = value(8)?;
                let timestamp = chrono::DateTime::parse_from_rfc3339(&date)
                    .map(|d| d.timestamp())
                    .or_else(|_| {
                        chrono::NaiveDateTime::parse_from_str(&date, "%Y-%m-%d %H:%M:%S%.f")
                            .map(|d| d.and_utc().timestamp())
                    })
                    .unwrap_or_default();
                let body_text = value(11)?;
                let body_html = value(12)?;
                Ok((
                    value(1)?,
                    value(2)?,
                    Message {
                        uid: row.get(0)?,
                        message_id: value(3)?.trim_matches(['<', '>']).to_string(),
                        subject: value(4)?,
                        sender: format!("{} <{}>", value(5)?, value(6)?),
                        recipients: value(7)?,
                        date,
                        timestamp,
                        references: value(9)?
                            .split_whitespace()
                            .map(|s| s.trim_matches(['<', '>']).to_string())
                            .collect(),
                        is_read: row.get::<_, Option<bool>>(10)?.unwrap_or(false),
                        is_flagged: row.get::<_, Option<bool>>(13)?.unwrap_or(false),
                        body_loaded: !body_text.is_empty() || !body_html.is_empty(),
                        body_text,
                        body_html,
                        attachments: Vec::new(),
                    },
                ))
            })?;
            for row in rows {
                let (account, folder, message) = row?;
                transaction.execute(
                    "INSERT OR IGNORE INTO rust_messages VALUES (?1,?2,?3,?4)",
                    params![
                        account,
                        folder,
                        message.uid,
                        serde_json::to_string(&message)?
                    ],
                )?;
                transaction.execute(
                    "INSERT OR IGNORE INTO rust_folders (account_id,folder) VALUES (?1,?2)",
                    params![account, folder],
                )?;
            }
            transaction.execute("INSERT INTO rust_metadata VALUES ('legacy_import','1')", [])?;
        }
        transaction.commit()?;
        Ok(Self(connection))
    }

    pub fn messages(&self, account: &str, folder: &str) -> Result<Vec<Message>> {
        let mut statement = self
            .0
            .prepare("SELECT data FROM rust_messages WHERE account_id=?1 AND folder=?2")?;
        let rows = statement.query_map(params![account, folder], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }

    pub fn message(&self, account: &str, folder: &str, uid: u32) -> Result<Option<Message>> {
        use rusqlite::OptionalExtension;
        let data: Option<String> = self
            .0
            .query_row(
                "SELECT data FROM rust_messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
                params![account, folder, uid],
                |row| row.get(0),
            )
            .optional()?;
        data.map(|json| Ok(serde_json::from_str(&json)?))
            .transpose()
    }

    pub fn store(&self, account: &str, folder: &str, message: &Message) -> Result<()> {
        self.0.execute(
            "INSERT OR REPLACE INTO rust_messages VALUES (?1,?2,?3,?4)",
            params![
                account,
                folder,
                message.uid,
                serde_json::to_string(message)?
            ],
        )?;
        Ok(())
    }

    pub fn folders(&self, account: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .0
            .prepare("SELECT folder FROM rust_folders WHERE account_id=?1 ORDER BY folder")?;
        Ok(stmt
            .query_map([account], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn store_folders(&mut self, account: &str, folders: &[String]) -> Result<()> {
        let transaction = self.0.transaction()?;
        for folder in folders {
            transaction.execute(
                "INSERT OR IGNORE INTO rust_folders (account_id,folder) VALUES (?1,?2)",
                params![account, folder],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn reconcile(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        messages: Vec<Message>,
        live: &[u32],
    ) -> Result<()> {
        let transaction = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Vec<Message> = {
            let mut statement = transaction
                .prepare("SELECT data FROM rust_messages WHERE account_id=?1 AND folder=?2")?;
            let rows =
                statement.query_map(params![account, folder], |row| row.get::<_, String>(0))?;
            rows.map(|row| Ok(serde_json::from_str(&row?)?))
                .collect::<Result<_>>()?
        };
        let previous = transaction.query_row(
            "SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2",
            params![account, folder],
            |r| r.get::<_, Option<u32>>(0),
        );
        let reset = !matches!(previous, Ok(Some(value)) if value == validity);
        if reset {
            transaction.execute(
                "DELETE FROM rust_messages WHERE account_id=?1 AND folder=?2",
                params![account, folder],
            )?;
        }
        let live: HashSet<_> = live.iter().copied().collect();
        for message in &existing {
            if !live.contains(&message.uid) {
                transaction.execute(
                    "DELETE FROM rust_messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
                    params![account, folder, message.uid],
                )?;
            }
        }
        for mut message in messages {
            if !reset && let Some(old) = existing.iter().find(|m| m.uid == message.uid) {
                message.body_text = old.body_text.clone();
                message.body_html = old.body_html.clone();
                message.attachments = old.attachments.clone();
                message.body_loaded = old.body_loaded;
            }
            transaction.execute(
                "INSERT OR REPLACE INTO rust_messages VALUES (?1,?2,?3,?4)",
                params![
                    account,
                    folder,
                    message.uid,
                    serde_json::to_string(&message)?
                ],
            )?;
        }
        transaction.execute(
            "INSERT OR REPLACE INTO rust_folders VALUES (?1,?2,?3)",
            params![account, folder, validity],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn storage() -> Storage {
        Storage::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn imports_legacy_cache_once_without_changing_original_rows() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE messages (
            uid INTEGER PRIMARY KEY, account_id TEXT, folder TEXT, message_id TEXT, subject TEXT,
            sender_name TEXT, sender_email TEXT, recipients TEXT, date_sent TEXT,
            message_references TEXT, is_read BOOLEAN, body_text TEXT, body_html TEXT, is_deleted BOOLEAN, is_flagged BOOLEAN);
            INSERT INTO messages VALUES (7,'a','INBOX','<id>','Legacy','Sender','sender@example.com','[]',
            '2026-09-09 12:00:00','<parent>',0,'Cached text','',0,1);").unwrap();
        let storage = Storage::initialize(connection).unwrap();
        let imported = storage.messages("a", "INBOX").unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].body_text, "Cached text");
        assert!(imported[0].body_loaded);
        assert!(imported[0].timestamp > 0);
        assert_eq!(imported[0].message_id, "id");
        storage.0.execute("DELETE FROM rust_messages", []).unwrap();
        let storage = Storage::initialize(storage.0).unwrap();
        assert!(storage.messages("a", "INBOX").unwrap().is_empty());
        let count: i32 = storage
            .0
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn isolates_uids_by_account_and_folder() {
        let storage = storage();
        let message = Message {
            uid: 1,
            subject: "Original".into(),
            ..Default::default()
        };
        storage.store("a", "INBOX", &message).unwrap();
        let other = Message {
            subject: "Other".into(),
            ..message
        };
        storage.store("b", "INBOX", &other).unwrap();
        storage.store("a", "Sent", &other).unwrap();
        assert_eq!(
            storage.messages("a", "INBOX").unwrap()[0].subject,
            "Original"
        );
        assert_eq!(storage.messages("b", "INBOX").unwrap()[0].subject, "Other");
    }

    #[test]
    fn reconciles_deletions_preserves_bodies_and_resets_uidvalidity() {
        let mut storage = storage();
        let header = Message {
            uid: 1,
            ..Default::default()
        };
        storage
            .reconcile("a", "INBOX", 1, vec![header.clone()], &[1])
            .unwrap();
        let body = Message {
            body_loaded: true,
            body_text: "Cached".into(),
            ..header.clone()
        };
        storage.store("a", "INBOX", &body).unwrap();
        storage
            .reconcile("a", "INBOX", 1, vec![header.clone()], &[1])
            .unwrap();
        assert!(storage.messages("a", "INBOX").unwrap()[0].body_loaded);
        storage
            .reconcile("a", "INBOX", 2, vec![header], &[1])
            .unwrap();
        assert!(!storage.messages("a", "INBOX").unwrap()[0].body_loaded);
        storage.reconcile("a", "INBOX", 2, vec![], &[]).unwrap();
        assert!(storage.messages("a", "INBOX").unwrap().is_empty());
    }
}
