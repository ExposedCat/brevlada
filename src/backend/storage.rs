use crate::models::{Message, SentMessage};
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::{collections::HashSet, path::Path, sync::Arc};

#[path = "storage_access.rs"]
mod access;
#[path = "action_storage.rs"]
mod actions;
#[path = "avatar_storage.rs"]
pub mod avatar;
#[path = "calendar_storage.rs"]
mod calendar;
#[cfg(test)]
#[path = "storage_concurrency.rs"]
mod concurrency_tests;
#[path = "sync_history.rs"]
mod history;
#[path = "read_storage.rs"]
mod reads;
#[path = "sender_storage.rs"]
mod senders;
#[path = "sync_settings.rs"]
mod settings;
#[path = "sync_storage.rs"]
mod sync;

pub struct Storage(Connection, Arc<access::Access>);

impl Storage {
    pub fn open_read_only(path: &Path) -> Result<Self> {
        let access = access::Access::for_path(path)?;
        // Wait for an initialization already in progress before using its schema.
        let _write = access.lock()?;
        let connection =
            Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self(connection, access.clone()))
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let access = access::Access::for_path(path)?;
        let mut state = access.lock()?;
        let connection = Connection::open(path)?;
        if !state.initialized {
            let mut storage = Self::initialize(connection)?;
            storage.1 = access.clone();
            state.initialized = true;
            Ok(storage)
        } else {
            connection.busy_timeout(std::time::Duration::from_secs(5))?;
            Ok(Self(connection, access.clone()))
        }
    }

    fn initialize(mut connection: Connection) -> Result<Self> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        // Take the SQLite write lock up front so external writers honor the busy
        // timeout instead of causing a failed read-to-write transaction upgrade.
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS rust_messages (
            account_id TEXT NOT NULL, folder TEXT NOT NULL, uid INTEGER NOT NULL,
            data TEXT NOT NULL, PRIMARY KEY(account_id, folder, uid));
            CREATE TABLE IF NOT EXISTS rust_removed_messages (
            account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER NOT NULL,
            uid INTEGER NOT NULL, PRIMARY KEY(account_id, folder, uid_validity, uid));
            CREATE TABLE IF NOT EXISTS rust_folders (
            account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER,
            PRIMARY KEY(account_id, folder));
            CREATE TABLE IF NOT EXISTS rust_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS rust_read_changes (
            account_id TEXT NOT NULL, folder TEXT NOT NULL, uid INTEGER NOT NULL,
            uid_validity INTEGER, revision INTEGER NOT NULL,
            PRIMARY KEY(account_id, folder, uid));
            CREATE TABLE IF NOT EXISTS rust_sync_settings (
            account_id TEXT PRIMARY KEY, folders TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS rust_avatars (
            email TEXT PRIMARY KEY, source TEXT NOT NULL, image BLOB,
            fetched_at INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS rust_unread (
            account_id TEXT NOT NULL, folder TEXT NOT NULL, unread BOOLEAN NOT NULL,
            PRIMARY KEY(account_id, folder));
            CREATE TABLE IF NOT EXISTS rust_sent_local (
            account_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL,
            PRIMARY KEY(account_id, message_id));
            CREATE INDEX IF NOT EXISTS rust_messages_date ON rust_messages
            (account_id, folder, json_extract(data, '$.timestamp') DESC, uid DESC);",
        )?;
        // Old cached bodies lack Reply-To and may also lack Cc. Reload them once
        // so responding to already cached mail uses the original headers.
        transaction.execute(
            "UPDATE rust_messages SET data=json_set(data, '$.body_loaded', json('false'))
             WHERE json_type(data, '$.reply_to') IS NULL",
            [],
        )?;
        // Older draft cache entries lack the recipient/reply fields needed by the editor.
        transaction.execute(
            "UPDATE rust_messages SET data=json_set(data, '$.body_loaded', json('false'))
             WHERE coalesce(json_extract(data, '$.is_draft'),0) AND json_type(data, '$.cc') IS NULL",
            [],
        )?;
        use rusqlite::OptionalExtension;
        let sources: Option<String> = transaction
            .query_row(
                "SELECT value FROM rust_metadata WHERE key='avatar_sources'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if sources.as_deref() != Some(super::avatars::SOURCES) {
            // The avatars were produced by a pipeline that no longer exists, so
            // both the images and the "this sender has none" records may be
            // wrong. They are cheap to rebuild, so drop the lot.
            transaction.execute("DELETE FROM rust_avatars", [])?;
            transaction.execute(
                "INSERT OR REPLACE INTO rust_metadata VALUES ('avatar_sources',?1)",
                params![super::avatars::SOURCES],
            )?;
        }
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
                        uid_validity: None,
                        message_id: value(3)?.trim_matches(['<', '>']).to_string(),
                        subject: value(4)?,
                        sender: format!("{} <{}>", value(5)?, value(6)?),
                        reply_to: String::new(),
                        recipients: value(7)?,
                        cc: String::new(),
                        in_reply_to: None,
                        date,
                        timestamp,
                        references: value(9)?
                            .split_whitespace()
                            .map(|s| s.trim_matches(['<', '>']).to_string())
                            .collect(),
                        is_read: row.get::<_, Option<bool>>(10)?.unwrap_or(false),
                        read_revision: 0,
                        flag_revision: 0,
                        is_spam: false,
                        is_flagged: row.get::<_, Option<bool>>(13)?.unwrap_or(false),
                        is_draft: false,
                        body_loaded: false,
                        body_text,
                        body_html,
                        attachments: Vec::new(),
                        attachment_details: Vec::new(),
                        inline_media: Vec::new(),
                        inline_media_loaded: false,
                        unsubscribe: None,
                        parcels: Vec::new(),
                        calendar_events: Vec::new(),
                        tickets: Vec::new(),
                        search_match: false,
                        list_preview: None,
                        display_prepared: false,
                        remote_media: None,
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
        Ok(Self(connection, access::Access::initialized()))
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

    pub fn delete_draft(
        &mut self,
        account: &str,
        target: &crate::models::draft::Target,
    ) -> Result<()> {
        if let Some(validity) = target.validity.filter(|_| target.uid != 0) {
            self.confirm_action(
                account,
                &target.folder,
                &[Message {
                    uid: target.uid,
                    uid_validity: Some(validity),
                    is_draft: true,
                    ..Default::default()
                }],
                crate::models::sender_action::SenderAction::Archive,
            )?;
        } else {
            let _write = self.1.lock()?;
            self.0.execute(
                "DELETE FROM rust_messages WHERE account_id=?1 AND folder=?2 AND json_extract(data, '$.message_id')=?3 AND coalesce(json_extract(data, '$.is_draft'),0)",
                params![account, target.folder, target.message_id],
            )?;
        }
        Ok(())
    }

    pub fn store_saved_draft(
        &mut self,
        account: &str,
        folder: &str,
        message: &Message,
    ) -> Result<()> {
        use rusqlite::{OptionalExtension, TransactionBehavior};
        let validity = message
            .uid_validity
            .context("Saved draft needs UIDVALIDITY")?;
        anyhow::ensure!(
            message.uid != 0 && message.is_draft && message.body_loaded,
            "Saved draft is incomplete"
        );
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<u32> = transaction
            .query_row(
                "SELECT uid_validity FROM rust_folders WHERE account_id=?1 AND folder=?2",
                params![account, folder],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        anyhow::ensure!(
            current.is_none_or(|current| current == validity),
            "Draft mailbox changed while caching"
        );
        anyhow::ensure!(
            !Self::removed_in(&transaction, account, folder, validity)?.contains(&message.uid),
            "Draft was already removed"
        );
        transaction.execute(
            "INSERT OR REPLACE INTO rust_folders VALUES (?1,?2,?3)",
            params![account, folder, validity],
        )?;
        transaction.execute(
            "INSERT OR REPLACE INTO rust_messages VALUES (?1,?2,?3,?4)",
            params![
                account,
                folder,
                message.uid,
                serde_json::to_string(message)?
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn store_sent(&self, account: &str, message: &Message) -> Result<()> {
        let _write = self.1.lock()?;
        self.0.execute(
            "INSERT OR REPLACE INTO rust_sent_local VALUES (?1,?2,?3)",
            params![account, message.message_id, serde_json::to_string(message)?],
        )?;
        Ok(())
    }

    pub fn related_sent(
        &self,
        account: &str,
        folder: &str,
        received: &[Message],
    ) -> Result<Vec<SentMessage>> {
        let mut candidates = Vec::new();
        let mut local = self
            .0
            .prepare("SELECT data FROM rust_sent_local WHERE account_id=?1")?;
        for row in local.query_map([account], |row| row.get::<_, String>(0))? {
            candidates.push(SentMessage {
                folder: String::new(),
                message: serde_json::from_str(&row?)?,
            });
        }
        let mut cached = self.0.prepare(
            "SELECT m.folder, m.data, f.uid_validity FROM rust_messages m
             LEFT JOIN rust_folders f ON m.account_id=f.account_id AND m.folder=f.folder
             WHERE m.account_id=?1 AND m.folder<>?2 ORDER BY json_extract(m.data, '$.timestamp') DESC",
        )?;
        for row in cached.query_map(params![account, folder], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<u32>>(2)?,
            ))
        })? {
            let (folder, data, validity) = row?;
            let mut message: Message = serde_json::from_str(&data)?;
            message.uid_validity = message.uid_validity.or(validity);
            super::mail_sync::ensure_draft_id(&mut message, &folder);
            if message.is_draft
                || crate::models::senders::key(&message).eq_ignore_ascii_case(account)
            {
                candidates.push(SentMessage { folder, message });
            }
        }
        Ok(crate::models::conversation::related(received, &candidates))
    }

    #[cfg(test)]
    pub fn store(&self, account: &str, folder: &str, message: &Message) -> Result<()> {
        let _write = self.1.lock()?;
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

    pub fn unread(&self, account: &str) -> Result<Vec<(String, bool)>> {
        let mut statement = self.0.prepare(
            "SELECT folder, unread FROM rust_unread WHERE account_id=?1
             UNION ALL
             SELECT folder, MAX(NOT json_extract(data, '$.is_read')) FROM rust_messages
             WHERE account_id=?1 AND folder NOT IN
                 (SELECT folder FROM rust_unread WHERE account_id=?1)
             GROUP BY folder",
        )?;
        Ok(statement
            .query_map([account], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn store_unread(&self, account: &str, folder: &str, unread: bool) -> Result<()> {
        let _write = self.1.lock()?;
        self.0.execute(
            "INSERT OR REPLACE INTO rust_unread VALUES (?1,?2,?3)",
            params![account, folder, unread],
        )?;
        Ok(())
    }

    pub fn store_folders(&mut self, account: &str, folders: &[String]) -> Result<()> {
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        for folder in folders {
            transaction.execute(
                "INSERT OR IGNORE INTO rust_folders (account_id,folder) VALUES (?1,?2)",
                params![account, folder],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    #[cfg(test)]
    pub fn reconcile(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        messages: Vec<Message>,
        live: &[u32],
    ) -> Result<()> {
        self.reconcile_since(
            account,
            folder,
            validity,
            messages,
            live,
            self.read_revision()?,
        )
    }

    pub fn reconcile_since(
        &mut self,
        account: &str,
        folder: &str,
        validity: u32,
        messages: Vec<Message>,
        live: &[u32],
        snapshot_revision: i64,
    ) -> Result<()> {
        let _write = self.1.lock()?;
        let transaction = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let revision = Self::next_read_revision(&transaction)?;
        let reads = Self::reads_since(&transaction, account, folder, validity, snapshot_revision)?;
        let starred =
            Self::flags_since(&transaction, account, folder, validity, snapshot_revision)?;
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
        let removed = Self::removed_in(&transaction, account, folder, validity)?;
        let live: HashSet<_> = live
            .iter()
            .copied()
            .filter(|uid| !removed.contains(uid))
            .collect();
        for message in &existing {
            if !live.contains(&message.uid) {
                transaction.execute(
                    "DELETE FROM rust_messages WHERE account_id=?1 AND folder=?2 AND uid=?3",
                    params![account, folder, message.uid],
                )?;
            }
        }
        for mut message in messages
            .into_iter()
            .filter(|message| !removed.contains(&message.uid))
        {
            message.uid_validity = Some(validity);
            message.is_read = reads.get(&message.uid).copied().unwrap_or(message.is_read);
            message.is_flagged = starred
                .get(&message.uid)
                .copied()
                .unwrap_or(message.is_flagged);
            message.read_revision = revision;
            message.flag_revision = revision;
            if !reset && let Some(old) = existing.iter().find(|m| m.uid == message.uid) {
                message.body_text = old.body_text.clone();
                message.body_html = old.body_html.clone();
                message.attachments = old.attachments.clone();
                message.attachment_details = old.attachment_details.clone();
                message.inline_media = old.inline_media.clone();
                message.inline_media_loaded = old.inline_media_loaded;
                message.body_loaded = old.body_loaded;
                message.parcels = old.parcels.clone();
                message.calendar_events = old.calendar_events.clone();
                message.tickets = old.tickets.clone();
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
    fn sender_headers_include_older_unread_mail_without_loading_heavy_bodies() {
        let storage = storage();
        for uid in 1..=120 {
            storage
                .store(
                    "a",
                    "INBOX",
                    &Message {
                        uid,
                        timestamp: uid as i64,
                        sender: if uid == 1 || uid == 120 {
                            "Alice <alice@example.com>".into()
                        } else {
                            format!("sender{uid}@example.com")
                        },
                        is_read: uid != 1 && uid != 2,
                        body_loaded: true,
                        body_text: "Searchable cached text".into(),
                        body_html: "<p>Cached body</p>".into(),
                        attachments: vec!["file.pdf".into()],
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        storage
            .store(
                "b",
                "INBOX",
                &Message {
                    uid: 121,
                    ..Default::default()
                },
            )
            .unwrap();
        storage
            .store(
                "a",
                "Archive",
                &Message {
                    uid: 122,
                    ..Default::default()
                },
            )
            .unwrap();
        let headers = storage.sender_headers("a", "INBOX").unwrap();
        assert_eq!(headers.len(), 120);
        assert!(headers.iter().all(|message| {
            !message.body_loaded && message.body_html.is_empty() && message.attachments.is_empty()
        }));
        assert!(headers.iter().all(|message| message.matches("searchable")));
        let groups = crate::models::senders::groups(&headers, "");
        assert_eq!(groups.len(), 119);
        assert_eq!(groups[0][0].uid, 120);
        assert!(groups[0].iter().any(|message| !message.is_read));
        assert_eq!(groups[1][0].uid, 2);
        let body = storage.message("a", "INBOX", 120).unwrap().unwrap();
        assert!(body.body_loaded);
        assert_eq!(body.body_html, "<p>Cached body</p>");
        assert_eq!(body.attachments, vec!["file.pdf"]);
    }

    #[test]
    fn sender_pages_are_bounded_and_continue_from_last_message() {
        let storage = storage();
        for uid in 1..=120 {
            storage
                .store(
                    "a",
                    "INBOX",
                    &Message {
                        uid,
                        timestamp: uid as i64,
                        sender: if uid % 2 == 0 {
                            "Alice <ALICE@example.com>"
                        } else {
                            "Bob <bob@example.com>"
                        }
                        .into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let first = storage
            .sender_page("a", "INBOX", "alice@example.com", None, 25)
            .unwrap();
        assert_eq!(first.len(), 25);
        assert_eq!(first[0].uid, 120);
        assert_eq!(first[24].uid, 72);
        let next = storage
            .sender_page("a", "INBOX", "alice@example.com", Some((72, 72)), 25)
            .unwrap();
        assert_eq!(next.len(), 25);
        assert_eq!(next[0].uid, 70);
        assert_eq!(next[24].uid, 22);
    }

    #[test]
    fn sender_page_skips_addresses_only_mentioned_in_display_names() {
        let storage = storage();
        for uid in 1..=60 {
            let sender = if uid > 30 {
                "alice@example.com <other@example.com>"
            } else {
                "Alice <alice@example.com>"
            };
            storage
                .store(
                    "a",
                    "INBOX",
                    &Message {
                        uid,
                        timestamp: uid as i64,
                        sender: sender.into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let page = storage
            .sender_page("a", "INBOX", "alice@example.com", None, 10)
            .unwrap();
        assert_eq!(page.len(), 10);
        assert_eq!(page[0].uid, 30);
        assert_eq!(page[9].uid, 21);
    }

    #[test]
    fn restores_unread_state_with_cached_messages_as_a_fallback() {
        let storage = storage();
        storage
            .store(
                "a",
                "INBOX",
                &Message {
                    uid: 1,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(storage.unread("a").unwrap(), vec![("INBOX".into(), true)]);
        // Server counts take precedence over an older cached message's flags.
        storage.store_unread("a", "INBOX", false).unwrap();
        storage.store_unread("a", "Archive", true).unwrap();
        storage.store_unread("b", "INBOX", true).unwrap();
        let reopened = Storage::initialize(storage.0).unwrap();
        let mut unread = reopened.unread("a").unwrap();
        unread.sort();
        assert_eq!(
            unread,
            vec![("Archive".into(), true), ("INBOX".into(), false)]
        );
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
        assert!(!imported[0].body_loaded);
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
    fn finds_locally_sent_reply_in_received_thread() {
        let storage = storage();
        let received = Message {
            uid: 7,
            message_id: "incoming".into(),
            sender: "other@example.com".into(),
            ..Default::default()
        };
        let sent = Message {
            message_id: "outgoing".into(),
            sender: "Me <me@example.com>".into(),
            references: vec!["incoming".into()],
            body_text: "Hello".into(),
            body_loaded: true,
            ..Default::default()
        };
        storage.store_sent("me@example.com", &sent).unwrap();
        let related = storage
            .related_sent("me@example.com", "INBOX", &[received])
            .unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].message, sent);
        assert!(related[0].folder.is_empty());
    }

    #[test]
    fn caches_one_saved_draft_without_removing_other_drafts_and_rejects_stale_validity() {
        let mut storage = storage();
        let existing = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "existing".into(),
            is_draft: true,
            body_loaded: true,
            body_text: "Keep this draft".into(),
            ..Default::default()
        };
        storage.store_saved_draft("a", "Drafts", &existing).unwrap();
        let saved = Message {
            uid: 2,
            message_id: "saved".into(),
            body_text: "New draft".into(),
            cc: "cc@example.com".into(),
            ..existing.clone()
        };
        storage.store_saved_draft("a", "Drafts", &saved).unwrap();
        assert_eq!(
            storage.message("a", "Drafts", 1).unwrap(),
            Some(existing.clone())
        );
        assert_eq!(
            storage.message("a", "Drafts", 2).unwrap(),
            Some(saved.clone())
        );
        let stale = Message {
            uid_validity: Some(8),
            ..saved
        };
        assert!(storage.store_saved_draft("a", "Drafts", &stale).is_err());
        assert_eq!(storage.validity("a", "Drafts").unwrap(), Some(7));
        assert_eq!(storage.message("a", "Drafts", 1).unwrap(), Some(existing));
    }

    #[test]
    fn injects_related_drafts_without_from_or_message_id_and_removes_deleted_drafts() {
        let mut storage = storage();
        let received = Message {
            uid: 7,
            message_id: "incoming".into(),
            subject: "Topic".into(),
            sender: "other@example.com".into(),
            ..Default::default()
        };
        let draft = Message {
            uid: 1,
            is_draft: true,
            recipients: "other@example.com".into(),
            subject: "Re: Topic".into(),
            ..Default::default()
        };
        storage.store("me@example.com", "Drafts", &draft).unwrap();
        storage.store("else@example.com", "Drafts", &draft).unwrap();
        storage
            .store(
                "me@example.com",
                "Drafts",
                &Message {
                    uid: 2,
                    message_id: "unrelated".into(),
                    subject: "Other topic".into(),
                    ..draft
                },
            )
            .unwrap();
        let related = storage
            .related_sent("me@example.com", "INBOX", std::slice::from_ref(&received))
            .unwrap();
        assert_eq!(related.len(), 1);
        assert_eq!(related[0].folder, "Drafts");
        assert!(related[0].message.is_draft);
        assert!(!related[0].message.message_id.is_empty());
        assert!(
            storage
                .related_sent("me@example.com", "Drafts", std::slice::from_ref(&received))
                .unwrap()
                .is_empty()
        );
        storage
            .reconcile("me@example.com", "Drafts", 1, vec![], &[])
            .unwrap();
        assert!(
            storage
                .related_sent("me@example.com", "INBOX", &[received])
                .unwrap()
                .is_empty()
        );
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
            tickets: vec![crate::models::ticket::Reservation {
                number: Some("booking-one".into()),
                name: "Concert".into(),
                ..Default::default()
            }],
            ..header.clone()
        };
        storage.store("a", "INBOX", &body).unwrap();
        storage
            .reconcile("a", "INBOX", 1, vec![header.clone()], &[1])
            .unwrap();
        assert!(storage.messages("a", "INBOX").unwrap()[0].body_loaded);
        assert_eq!(
            storage.messages("a", "INBOX").unwrap()[0].tickets,
            body.tickets
        );
        storage
            .reconcile("a", "INBOX", 2, vec![header], &[1])
            .unwrap();
        assert!(!storage.messages("a", "INBOX").unwrap()[0].body_loaded);
        assert!(
            storage.messages("a", "INBOX").unwrap()[0]
                .tickets
                .is_empty()
        );
        storage.reconcile("a", "INBOX", 2, vec![], &[]).unwrap();
        assert!(storage.messages("a", "INBOX").unwrap().is_empty());
    }

    #[test]
    fn reloads_old_cached_headers_and_persists_reply_to_from_fetched_body() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE rust_messages (
            account_id TEXT, folder TEXT, uid INTEGER, data TEXT,
            PRIMARY KEY(account_id, folder, uid));",
            )
            .unwrap();
        let old = Message {
            uid: 1,
            message_id: "incoming".into(),
            body_loaded: true,
            body_text: "Cached".into(),
            ..Default::default()
        };
        let mut data = serde_json::to_value(&old).unwrap();
        data.as_object_mut().unwrap().remove("reply_to");
        connection
            .execute(
                "INSERT INTO rust_messages VALUES ('a','INBOX',1,?1)",
                [data.to_string()],
            )
            .unwrap();
        let mut storage = Storage::initialize(connection).unwrap();
        let cached = storage.message("a", "INBOX", 1).unwrap().unwrap();
        assert!(!cached.body_loaded);
        assert_eq!(cached.body_text, "Cached");
        let fetched = Message {
            reply_to: "support@example.com".into(),
            cc: "copy@example.com".into(),
            ..old
        };
        let refreshed = storage
            .store_body("a", "INBOX", None, &fetched)
            .unwrap()
            .unwrap();
        assert!(refreshed.body_loaded);
        assert_eq!(refreshed.reply_to, "support@example.com");
        assert_eq!(refreshed.cc, "copy@example.com");
        assert_eq!(
            storage.message("a", "INBOX", 1).unwrap().unwrap(),
            refreshed
        );
    }
}
