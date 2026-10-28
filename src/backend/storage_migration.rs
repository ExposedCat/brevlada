//! Creates the cache schema and moves older caches into it once.
use super::messages::{self, Mailbox};
use crate::models::Message;
use anyhow::Result;
use rusqlite::{Transaction, params};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS rust_mailboxes (
    id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, folder TEXT NOT NULL,
    UNIQUE(account_id, folder));
    CREATE TABLE IF NOT EXISTS rust_headers (
    id INTEGER PRIMARY KEY, account_id TEXT NOT NULL, folder TEXT, sender_key TEXT NOT NULL,
    uid INTEGER NOT NULL, uid_validity INTEGER, message_id TEXT NOT NULL, subject TEXT NOT NULL,
    sender TEXT NOT NULL, reply_to TEXT NOT NULL, recipients TEXT NOT NULL, cc TEXT NOT NULL,
    in_reply_to TEXT, date TEXT NOT NULL, timestamp INTEGER NOT NULL, \"references\" TEXT NOT NULL,
    is_read BOOLEAN NOT NULL, read_revision INTEGER NOT NULL, is_draft BOOLEAN NOT NULL,
    is_flagged BOOLEAN NOT NULL, is_spam BOOLEAN NOT NULL, flag_revision INTEGER NOT NULL,
    attachments TEXT NOT NULL, attachment_details TEXT NOT NULL,
    inline_media_loaded BOOLEAN NOT NULL, unsubscribe TEXT NOT NULL, parcels TEXT NOT NULL,
    calendar_events TEXT NOT NULL, tickets TEXT NOT NULL, body_loaded BOOLEAN NOT NULL);
    CREATE INDEX IF NOT EXISTS rust_headers_date
    ON rust_headers (account_id, folder, timestamp DESC, uid DESC, sender_key);
    CREATE UNIQUE INDEX IF NOT EXISTS rust_headers_sent
    ON rust_headers (account_id, message_id) WHERE folder IS NULL;
    CREATE INDEX IF NOT EXISTS rust_headers_body_copy
    ON rust_headers (account_id, trim(message_id, '<> ' || char(9) || char(10) || char(13)))
    WHERE body_loaded AND NOT is_draft;
    CREATE TABLE IF NOT EXISTS rust_conversation_keys (
    header_id INTEGER NOT NULL, account_id TEXT NOT NULL, kind INTEGER NOT NULL, value TEXT NOT NULL,
    PRIMARY KEY(header_id, kind, value));
    CREATE INDEX IF NOT EXISTS rust_conversation_lookup
    ON rust_conversation_keys (account_id, kind, value, header_id);
    CREATE TRIGGER IF NOT EXISTS rust_conversation_deleted AFTER DELETE ON rust_headers
    BEGIN DELETE FROM rust_conversation_keys WHERE header_id=old.id; END;
    -- List rows read the leading columns without walking the overflow pages of HTML bodies.
    CREATE TABLE IF NOT EXISTS rust_bodies (
    id INTEGER PRIMARY KEY, html_tickets TEXT NOT NULL, body_text TEXT NOT NULL,
    html_text TEXT NOT NULL, body_html TEXT NOT NULL, inline_media TEXT NOT NULL);
    CREATE TRIGGER IF NOT EXISTS rust_headers_deleted AFTER DELETE ON rust_headers
    BEGIN DELETE FROM rust_bodies WHERE id=old.id; END;
    CREATE TABLE IF NOT EXISTS rust_removed_messages (
    account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER NOT NULL,
    uid INTEGER NOT NULL, PRIMARY KEY(account_id, folder, uid_validity, uid));
    CREATE TABLE IF NOT EXISTS rust_folders (
    account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER,
    PRIMARY KEY(account_id, folder));
    CREATE TABLE IF NOT EXISTS rust_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS rust_notification_folders (
    account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER NOT NULL,
    last_uid INTEGER NOT NULL, PRIMARY KEY(account_id,folder));
    CREATE TABLE IF NOT EXISTS rust_notification_pending (
    account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER NOT NULL,
    uid INTEGER NOT NULL, PRIMARY KEY(account_id,folder,uid_validity,uid));
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
    PRIMARY KEY(account_id, folder));";

pub(super) fn run(transaction: &Transaction<'_>) -> Result<()> {
    transaction.execute_batch(SCHEMA)?;
    if exists(transaction, "rust_messages")? {
        import_messages(transaction)?;
    }
    if exists(transaction, "rust_sent_local")? {
        import_sent(transaction)?;
    }
    if exists(transaction, "messages")? && !legacy_imported(transaction)? {
        import_legacy(transaction)?;
    }
    super::conversation::migrate(transaction)?;
    Ok(())
}

fn exists(transaction: &Transaction<'_>, table: &str) -> Result<bool> {
    Ok(transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |row| row.get(0),
    )?)
}

/// Earlier versions kept each message as one JSON document.
fn import_messages(transaction: &Transaction<'_>) -> Result<()> {
    {
        // Old cached bodies lack Reply-To and may also lack Cc. Reload them once
        // so responding to already cached mail uses the original headers, and
        // so the editor gets the recipient fields of older drafts.
        let mut statement = transaction.prepare(
            "SELECT account_id, folder, data, json_type(data, '$.reply_to') IS NULL
             OR (coalesce(json_extract(data, '$.is_draft'), 0) AND json_type(data, '$.cc') IS NULL)
             FROM rust_messages",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let account: String = row.get(0)?;
            let folder: String = row.get(1)?;
            let mut message: Message = serde_json::from_str(row.get_ref(2)?.as_str()?)?;
            message.body_loaded &= !row.get::<_, bool>(3)?;
            let mailbox = Mailbox::ensure(transaction, &account, &folder)?;
            messages::add(
                transaction,
                mailbox.id(message.uid),
                &account,
                Some(&folder),
                &message,
            )?;
        }
    }
    transaction.execute_batch("DROP TABLE rust_messages")?;
    Ok(())
}

fn import_sent(transaction: &Transaction<'_>) -> Result<()> {
    {
        let mut statement = transaction.prepare("SELECT account_id, data FROM rust_sent_local")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let account: String = row.get(0)?;
            let message: Message = serde_json::from_str(row.get_ref(1)?.as_str()?)?;
            let id = messages::sent_id(transaction, &account, &message.message_id)?;
            messages::add(transaction, id, &account, None, &message)?;
        }
    }
    transaction.execute_batch("DROP TABLE rust_sent_local")?;
    Ok(())
}

fn legacy_imported(transaction: &Transaction<'_>) -> Result<bool> {
    Ok(transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM rust_metadata WHERE key='legacy_import')",
        [],
        |row| row.get(0),
    )?)
}

fn import_legacy(transaction: &Transaction<'_>) -> Result<()> {
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
                search_rank: 0,
                list_recipient: None,
                list_preview: None,
                display_prepared: false,
                remote_media: None,
            },
        ))
    })?;
    for row in rows {
        let (account, folder, message): (String, String, Message) = row?;
        let mailbox = Mailbox::ensure(transaction, &account, &folder)?;
        messages::add(
            transaction,
            mailbox.id(message.uid),
            &account,
            Some(&folder),
            &message,
        )?;
        transaction.execute(
            "INSERT OR IGNORE INTO rust_folders (account_id,folder) VALUES (?1,?2)",
            params![account, folder],
        )?;
    }
    transaction.execute("INSERT INTO rust_metadata VALUES ('legacy_import','1')", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::Storage;
    use crate::models::{
        InlineMedia, Message, Unsubscribe, attachment, calendar, parcel::Parcel, ticket,
    };
    use rusqlite::{Connection, params};

    /// The schema of caches written before messages were split into columns.
    const JSON_SCHEMA: &str = "CREATE TABLE rust_messages (
        account_id TEXT NOT NULL, folder TEXT NOT NULL, uid INTEGER NOT NULL,
        data TEXT NOT NULL, PRIMARY KEY(account_id, folder, uid));
        CREATE TABLE rust_removed_messages (
        account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER NOT NULL,
        uid INTEGER NOT NULL, PRIMARY KEY(account_id, folder, uid_validity, uid));
        CREATE TABLE rust_folders (
        account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER,
        PRIMARY KEY(account_id, folder));
        CREATE TABLE rust_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE rust_notification_folders (
        account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER NOT NULL,
        last_uid INTEGER NOT NULL, PRIMARY KEY(account_id,folder));
        CREATE TABLE rust_notification_pending (
        account_id TEXT NOT NULL, folder TEXT NOT NULL, uid_validity INTEGER NOT NULL,
        uid INTEGER NOT NULL, PRIMARY KEY(account_id,folder,uid_validity,uid));
        CREATE TABLE rust_read_changes (
        account_id TEXT NOT NULL, folder TEXT NOT NULL, uid INTEGER NOT NULL,
        uid_validity INTEGER, revision INTEGER NOT NULL,
        PRIMARY KEY(account_id, folder, uid));
        CREATE TABLE rust_sync_settings (account_id TEXT PRIMARY KEY, folders TEXT NOT NULL);
        CREATE TABLE rust_avatars (
        email TEXT PRIMARY KEY, source TEXT NOT NULL, image BLOB, fetched_at INTEGER NOT NULL);
        CREATE TABLE rust_unread (
        account_id TEXT NOT NULL, folder TEXT NOT NULL, unread BOOLEAN NOT NULL,
        PRIMARY KEY(account_id, folder));
        CREATE TABLE rust_sent_local (
        account_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL,
        PRIMARY KEY(account_id, message_id));
        CREATE INDEX rust_messages_date ON rust_messages
        (account_id, folder, json_extract(data, '$.timestamp') DESC, uid DESC);";

    fn event() -> calendar::Event {
        calendar::Event {
            uid: Some("meeting".into()),
            sequence: 2,
            method: Some("REQUEST".into()),
            organizer_email: Some("ada@example.com".into()),
            name: "Planning".into(),
            ..Default::default()
        }
    }

    fn complete() -> Message {
        Message {
            uid: 7,
            uid_validity: Some(3),
            message_id: "complete".into(),
            subject: "Every field".into(),
            sender: "Ada <ada@example.com>".into(),
            reply_to: "replies@example.com".into(),
            recipients: "me@example.com".into(),
            cc: "copy@example.com".into(),
            in_reply_to: Some("parent".into()),
            date: "Fri, 9 Oct 2026 10:00:00 +0000".into(),
            timestamp: 1_791_540_000,
            references: vec!["root".into(), "parent".into()],
            is_read: true,
            read_revision: 4,
            is_flagged: true,
            is_spam: true,
            flag_revision: 5,
            body_text: "Plain body".into(),
            body_html: "<p>HTML <b>body</b></p>".into(),
            attachments: vec!["invite.ics".into()],
            attachment_details: vec![attachment::Details {
                mime: "text/calendar".into(),
                size: 120,
                calendar_rendered: Some(true),
            }],
            inline_media: vec![InlineMedia {
                content_id: "logo".into(),
                mime: "image/png".into(),
                data: "aW1hZ2U=".into(),
            }],
            inline_media_loaded: true,
            unsubscribe: Some(Unsubscribe {
                url: "https://example.com/leave".into(),
                one_click: Some(true),
            }),
            parcels: vec![Parcel {
                item: Some("Lamp".into()),
                arrival_from: None,
                arrival_until: Some("2026-10-12".into()),
                carrier: Some("Courier".into()),
                tracking_url: None,
                order_url: None,
                order_number: Some("42".into()),
                steps: Vec::new(),
                details: serde_json::json!({"note": "fragile"}),
            }],
            calendar_events: vec![event()],
            tickets: vec![ticket::Reservation {
                name: "Concert".into(),
                number: Some("booking".into()),
                ..Default::default()
            }],
            body_loaded: true,
            ..Default::default()
        }
    }

    fn json_cache() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(JSON_SCHEMA).unwrap();
        connection
    }

    fn insert(connection: &Connection, account: &str, folder: &str, data: &serde_json::Value) {
        connection
            .execute(
                "INSERT INTO rust_messages VALUES (?1,?2,?3,?4)",
                params![account, folder, data["uid"].as_i64(), data.to_string()],
            )
            .unwrap();
    }

    #[test]
    fn moves_json_messages_and_local_sends_into_columns_without_losing_fields() {
        let connection = json_cache();
        let complete = complete();
        insert(
            &connection,
            "a",
            "INBOX",
            &serde_json::to_value(&complete).unwrap(),
        );
        let other = Message {
            uid: 7,
            subject: "Same UID elsewhere".into(),
            ..Default::default()
        };
        let data = serde_json::to_value(&other).unwrap();
        insert(&connection, "a", "Archive", &data);
        insert(&connection, "b", "INBOX", &data);
        let sent = Message {
            message_id: "sent".into(),
            sender: "me@example.com".into(),
            references: vec!["complete".into()],
            body_text: "Reply".into(),
            body_loaded: true,
            calendar_events: vec![calendar::Event {
                method: Some("REPLY".into()),
                ..event()
            }],
            ..Default::default()
        };
        connection
            .execute(
                "INSERT INTO rust_sent_local VALUES ('a','sent',?1)",
                [serde_json::to_string(&sent).unwrap()],
            )
            .unwrap();
        connection
            .execute_batch(&format!(
                "INSERT INTO rust_folders VALUES ('a','INBOX',3);
                 INSERT INTO rust_removed_messages VALUES ('a','INBOX',3,99);
                 INSERT INTO rust_metadata VALUES ('read_revision','9');
                 INSERT INTO rust_metadata VALUES ('avatar_sources','{}');
                 INSERT INTO rust_metadata VALUES ('last_sync_started','1800000000');
                 INSERT INTO rust_notification_folders VALUES ('a','INBOX',3,6);
                 INSERT INTO rust_notification_pending VALUES ('a','INBOX',3,7);
                 INSERT INTO rust_read_changes VALUES ('a','INBOX',7,3,8);
                 INSERT INTO rust_sync_settings VALUES ('a','[\"INBOX\",\"Archive\"]');
                 INSERT INTO rust_avatars VALUES ('ada@example.com','google',x'01',10);
                 INSERT INTO rust_unread VALUES ('a','Archive',1);",
                crate::backend::avatars::SOURCES
            ))
            .unwrap();
        let mut storage = Storage::initialize(connection).unwrap();
        for _ in 0..2 {
            assert_eq!(
                storage.message("a", "INBOX", 7).unwrap(),
                Some(complete.clone())
            );
            assert_eq!(
                storage.message("a", "Archive", 7).unwrap(),
                Some(other.clone())
            );
            assert_eq!(
                storage.message("b", "INBOX", 7).unwrap(),
                Some(other.clone())
            );
            let related = storage
                .related_sent("a", "INBOX", std::slice::from_ref(&complete))
                .unwrap();
            assert_eq!(related.len(), 1);
            assert_eq!(related[0].message, sent);
            assert_eq!(
                storage.calendar_messages("a", "meeting").unwrap(),
                [complete.clone(), sent.clone()]
            );
            assert_eq!(storage.read_revision().unwrap(), 9);
            assert_eq!(storage.validity("a", "INBOX").unwrap(), Some(3));
            assert_eq!(storage.folders("a").unwrap(), ["INBOX"]);
            assert_eq!(
                storage.unread("a").unwrap(),
                [("Archive".into(), true), ("INBOX".into(), false)]
            );
            assert_eq!(storage.sync_folders("a").unwrap(), ["INBOX", "Archive"]);
            assert_eq!(
                storage.avatar("ada@example.com").unwrap().unwrap().image,
                Some(vec![1])
            );
            assert_eq!(
                storage.last_sync_started().unwrap(),
                Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000))
            );
            assert_eq!(
                storage.notification_folders().unwrap(),
                [("a".into(), "INBOX".into(), 3)]
            );
            assert!(
                storage
                    .pending_notifications("a", "INBOX", 3)
                    .unwrap()
                    .is_empty()
            );
            let tables: i64 = storage
                .0
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name IN ('rust_messages','rust_sent_local')",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(tables, 0);
            storage = Storage::initialize(storage.0).unwrap();
        }
        assert!(
            storage
                .store_headers(
                    "a",
                    "INBOX",
                    3,
                    &[Message {
                        uid: 99,
                        ..Default::default()
                    }]
                )
                .is_ok()
        );
        assert!(storage.message("a", "INBOX", 99).unwrap().is_none());
    }

    #[test]
    fn reloads_old_bodies_missing_reply_to_and_old_drafts_missing_cc() {
        let connection = json_cache();
        let cached = Message {
            uid: 1,
            message_id: "cached".into(),
            body_loaded: true,
            body_text: "Cached".into(),
            ..Default::default()
        };
        let mut data = serde_json::to_value(&cached).unwrap();
        data.as_object_mut().unwrap().remove("reply_to");
        insert(&connection, "a", "INBOX", &data);
        let draft = Message {
            uid: 2,
            is_draft: true,
            ..cached.clone()
        };
        let mut data = serde_json::to_value(&draft).unwrap();
        data.as_object_mut().unwrap().remove("cc");
        insert(&connection, "a", "Drafts", &data);
        let current = Message {
            uid: 3,
            ..cached.clone()
        };
        insert(
            &connection,
            "a",
            "INBOX",
            &serde_json::to_value(&current).unwrap(),
        );
        let storage = Storage::initialize(connection).unwrap();
        assert_eq!(
            storage.message("a", "INBOX", 1).unwrap(),
            Some(Message {
                body_loaded: false,
                ..cached
            })
        );
        assert_eq!(
            storage.message("a", "Drafts", 2).unwrap(),
            Some(Message {
                body_loaded: false,
                ..draft
            })
        );
        assert_eq!(storage.message("a", "INBOX", 3).unwrap(), Some(current));
        assert_eq!(storage.uncached_uids("a", "INBOX").unwrap(), [1]);
    }

    #[test]
    fn reading_a_json_cache_moves_it_first() {
        let directory = std::env::temp_dir().join(format!(
            "brevlada-migration-{}",
            gtk::glib::uuid_string_random()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("cache.db");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(JSON_SCHEMA).unwrap();
        insert(
            &connection,
            "a",
            "INBOX",
            &serde_json::to_value(complete()).unwrap(),
        );
        drop(connection);
        let storage = Storage::open_read_only(&path).unwrap();
        assert_eq!(storage.message("a", "INBOX", 7).unwrap(), Some(complete()));
        assert_eq!(storage.sender_headers("a", "INBOX").unwrap().len(), 1);
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
