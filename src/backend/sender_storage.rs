use super::Storage;
use crate::models::Message;
use anyhow::Result;
use rusqlite::params;

impl Storage {
    pub(in crate::backend) fn search_folders(&self, account: &str) -> Result<Vec<String>> {
        let mut statement = self.0.prepare(
            "SELECT folder FROM rust_folders WHERE account_id=?1
             UNION SELECT folder FROM rust_messages WHERE account_id=?1 ORDER BY folder",
        )?;
        Ok(statement
            .query_map([account], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn parcel_sender_messages(
        &self,
        account: &str,
        folder: &str,
        sender: &str,
        identifier: &str,
    ) -> Result<Vec<Message>> {
        if sender.is_empty() || identifier.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self.0.prepare(
            "SELECT data FROM rust_messages WHERE account_id=?1 AND folder=?2
             AND instr(lower(json_extract(data, '$.sender')), ?3) > 0
             ORDER BY json_extract(data, '$.timestamp'), uid",
        )?;
        let rows = statement.query_map(params![account, folder, sender], |row| {
            row.get::<_, String>(0)
        })?;
        let messages = rows
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<Result<Vec<Message>>>()?;
        Ok(messages
            .into_iter()
            .filter(|message| crate::models::senders::key(message) == sender)
            .collect())
    }

    pub(in crate::backend) fn search_data_version(&self) -> Result<i64> {
        Ok(self
            .0
            .query_row("PRAGMA data_version", [], |row| row.get(0))?)
    }

    /// Visit cached mail without decoding attachments or parsing HTML up front.
    /// Returning false stops the SQLite scan immediately when a query is canceled.
    pub(in crate::backend) fn visit_search_messages(
        &self,
        account: &str,
        folder: &str,
        mut visit: impl FnMut(Message) -> bool,
    ) -> Result<()> {
        let mut statement = self.0.prepare(
            "SELECT uid,
             json_extract(data, '$.message_id'), json_extract(data, '$.subject'),
             json_extract(data, '$.sender'), json_extract(data, '$.reply_to'),
             json_extract(data, '$.recipients'), json_extract(data, '$.cc'),
             json_extract(data, '$.timestamp'), json_extract(data, '$.date'),
             json_extract(data, '$.is_read'), json_extract(data, '$.is_flagged'),
             json_extract(data, '$.body_text'), json_extract(data, '$.body_html'),
             json_extract(data, '$.uid_validity'), json_extract(data, '$.read_revision'),
             json_extract(data, '$.flag_revision')
             FROM rust_messages WHERE account_id=?1 AND folder=?2
             ORDER BY json_extract(data, '$.timestamp') DESC, uid DESC",
        )?;
        let mut rows = statement.query(params![account, folder])?;
        while let Some(row) = rows.next()? {
            let text = |column| -> rusqlite::Result<String> {
                Ok(row.get::<_, Option<String>>(column)?.unwrap_or_default())
            };
            let message = Message {
                uid: row.get(0)?,
                message_id: text(1)?,
                subject: text(2)?,
                sender: text(3)?,
                reply_to: text(4)?,
                recipients: text(5)?,
                cc: text(6)?,
                timestamp: row.get::<_, Option<i64>>(7)?.unwrap_or_default(),
                date: text(8)?,
                is_read: row.get::<_, Option<bool>>(9)?.unwrap_or_default(),
                is_flagged: row.get::<_, Option<bool>>(10)?.unwrap_or_default(),
                body_text: text(11)?,
                body_html: text(12)?,
                uid_validity: row.get(13)?,
                read_revision: row.get::<_, Option<i64>>(14)?.unwrap_or_default(),
                flag_revision: row.get::<_, Option<i64>>(15)?.unwrap_or_default(),
                ..Default::default()
            };
            if !visit(message) {
                break;
            }
        }
        Ok(())
    }

    pub fn sender_headers(&self, account: &str, folder: &str) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(
            "SELECT json_set(data, '$.inline_media', json('[]'), '$.parcels',
             CASE WHEN json_array_length(data, '$.calendar_events')>0
                  THEN coalesce(json_extract(data, '$.parcels'), json('[]')) ELSE json('[]') END,
             '$.attachments', CASE WHEN json_array_length(data, '$.calendar_events')>0
                  THEN coalesce(json_extract(data, '$.attachments'), json('[]')) ELSE json('[]') END,
             '$.body_loaded', json('false'), '$.inline_media_loaded', json('false'))
             FROM rust_messages WHERE account_id=?1 AND folder=?2
             ORDER BY json_extract(data, '$.timestamp') DESC, uid DESC",
        )?;
        let rows = statement.query_map(params![account, folder], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let mut message: Message = serde_json::from_str(&row?)?;
            if message.tickets.is_empty() {
                message.tickets = crate::models::ticket::parse(&message.body_html);
            }
            if !message.body_html.is_empty() {
                let html = crate::models::search::html_text(&message.body_html);
                if !html.is_empty() && !message.body_text.contains(&html) {
                    message.body_text.push('\n');
                    message.body_text.push_str(&html);
                }
                message.body_html.clear();
            }
            Ok(message)
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_messages_preserve_mailbox_identity_and_flag_revisions() {
        let mut storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let original = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "search-match".into(),
            is_flagged: true,
            flag_revision: 4,
            ..Default::default()
        };
        storage.store("a", "INBOX", &original).unwrap();
        let mut matches = Vec::new();
        storage
            .visit_search_messages("a", "INBOX", |message| {
                matches.push(message);
                true
            })
            .unwrap();
        assert_eq!(matches[0].uid_validity, original.uid_validity);
        assert_eq!(matches[0].flag_revision, original.flag_revision);
        let read = storage
            .mark_read_cached("a", "INBOX", None, 1)
            .unwrap()
            .unwrap();
        crate::models::read_state::merge(&mut matches, std::iter::once(&read));
        assert!(matches[0].is_read);
        assert_eq!(matches[0].read_revision, read.read_revision);
        storage
            .visit_search_messages("a", "INBOX", |message| {
                assert_eq!(message.read_revision, read.read_revision);
                assert!(message.is_read);
                false
            })
            .unwrap();
    }

    #[test]
    fn sender_headers_hydrate_ticket_payloads_before_trimming_html() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let message = Message {
            uid: 1,
            sender: "sender@example.com".into(),
            body_loaded: true,
            body_html: "<script type='application/ld+json'>{\"@type\":\"EventReservation\",\"reservationNumber\":\"booking-one\",\"reservationFor\":{\"@type\":\"Event\",\"name\":\"Concert\",\"startDate\":\"2026-10-08T20:00:00+02:00\"},\"reservedTicket\":{\"@type\":\"Ticket\",\"ticketNumber\":\"ticket-one\"}}</script>".into(),
            ..Default::default()
        };
        storage.store("me@example.com", "INBOX", &message).unwrap();
        let headers = storage.sender_headers("me@example.com", "INBOX").unwrap();
        assert_eq!(headers.len(), 1);
        assert!(headers[0].body_html.is_empty());
        assert_eq!(headers[0].tickets.len(), 1);
        assert_eq!(headers[0].tickets[0].number.as_deref(), Some("booking-one"));
        assert_eq!(
            headers[0].tickets[0].tickets[0].number.as_deref(),
            Some("ticket-one")
        );
    }

    #[test]
    fn sender_pages_prepare_previews_without_copying_inline_images() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let original = Message {
            uid: 1,
            sender: "sender@example.com".into(),
            body_loaded: true,
            body_html: "<p>Cached HTML preview</p>".into(),
            inline_media_loaded: true,
            inline_media: vec![crate::models::InlineMedia {
                content_id: "image".into(),
                mime: "image/png".into(),
                data: "x".repeat(1024 * 1024),
            }],
            ..Default::default()
        };
        storage.store("a", "INBOX", &original).unwrap();
        let page = storage
            .sender_page("a", "INBOX", "sender@example.com", None, 25)
            .unwrap();
        assert_eq!(page[0].list_preview.as_deref(), Some("Cached HTML preview"));
        assert!(page[0].display_prepared && page[0].body_loaded);
        assert!(page[0].inline_media.is_empty());
        assert_eq!(storage.message("a", "INBOX", 1).unwrap().unwrap(), original);
    }

    #[test]
    fn headers_keep_searchable_html_only_text_and_recipients_without_html_payloads() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        storage
            .store(
                "a",
                "INBOX",
                &Message {
                    uid: 1,
                    sender: "sender@example.com".into(),
                    recipients: "receiver@example.com".into(),
                    cc: "copy@example.com".into(),
                    body_loaded: true,
                    body_html: "<style>hidden</style><p>De<span>liv</span>ery &amp; status</p>"
                        .into(),
                    ..Default::default()
                },
            )
            .unwrap();
        let messages = storage.sender_headers("a", "INBOX").unwrap();
        assert!(messages[0].body_html.is_empty());
        for query in ["sender", "receiver", "copy", "delivery & status"] {
            assert!(messages[0].matches(query));
        }
        assert!(!messages[0].matches("hidden"));
    }

    #[test]
    fn history_includes_older_sender_mail_and_isolates_account_folder_and_address() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        for uid in 1..=60 {
            storage
                .store(
                    "a",
                    "INBOX",
                    &Message {
                        uid,
                        timestamp: uid as i64,
                        sender: "Shop <shop@example.com>".into(),
                        body_loaded: true,
                        body_html: "Delivery details".into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        for (account, folder, sender) in [
            ("b", "INBOX", "shop@example.com"),
            ("a", "Archive", "shop@example.com"),
            ("a", "INBOX", "other-shop@example.com"),
            ("a", "INBOX", "shop@example.com <other@example.com>"),
        ] {
            storage
                .store(
                    account,
                    folder,
                    &Message {
                        uid: 100,
                        sender: sender.into(),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let history = storage
            .parcel_sender_messages("a", "INBOX", "shop@example.com", "123")
            .unwrap();
        assert_eq!(history.len(), 60);
        assert_eq!(history[0].uid, 1);
        assert_eq!(history[0].body_html, "Delivery details");
        assert!(
            storage
                .parcel_sender_messages("a", "INBOX", "shop@example.com", " ")
                .unwrap()
                .is_empty()
        );
    }
}
