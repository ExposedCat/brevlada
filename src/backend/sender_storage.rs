use super::{Storage, messages};
use crate::models::Message;
use anyhow::Result;
use rusqlite::params;

impl Storage {
    pub(in crate::backend) fn search_folders(&self, account: &str) -> Result<Vec<String>> {
        let mut statement = self.0.prepare(
            "SELECT folder FROM rust_folders WHERE account_id=?1
             UNION SELECT folder FROM rust_headers WHERE account_id=?1 AND folder IS NOT NULL
             ORDER BY folder",
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
        let mut statement = self.0.prepare(concat!(
            messages::select!(),
            " WHERE h.account_id=?1 AND h.folder=?2 AND h.sender_key=?3
             AND instr(lower(h.sender), ?3) > 0 ORDER BY h.timestamp, h.uid"
        ))?;
        Ok(statement
            .query_map(params![account, folder, sender], messages::decode)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub(in crate::backend) fn search_data_version(&self) -> Result<i64> {
        Ok(self
            .0
            .query_row("PRAGMA data_version", [], |row| row.get(0))?)
    }

    /// Visit cached mail as list rows, with the text of HTML bodies extracted when
    /// they were stored. Returning false stops the SQLite scan immediately when a
    /// query is canceled.
    pub(in crate::backend) fn visit_search_messages(
        &self,
        account: &str,
        folder: &str,
        mut visit: impl FnMut(Message) -> bool,
    ) -> Result<()> {
        let mut statement = self.0.prepare(
            "SELECT h.uid, h.message_id, h.subject, h.sender, h.reply_to, h.recipients, h.cc,
             h.timestamp, h.date, h.is_read, h.is_flagged, coalesce(b.body_text, ''),
             coalesce(b.html_text, ''), h.uid_validity, h.read_revision, h.flag_revision
             FROM rust_headers h LEFT JOIN rust_bodies b ON b.id=h.id
             WHERE h.account_id=?1 AND h.folder=?2 ORDER BY h.timestamp DESC, h.uid DESC",
        )?;
        let mut rows = statement.query(params![account, folder])?;
        while let Some(row) = rows.next()? {
            let mut message = Message {
                uid: row.get(0)?,
                message_id: row.get(1)?,
                subject: row.get(2)?,
                sender: row.get(3)?,
                reply_to: row.get(4)?,
                recipients: row.get(5)?,
                cc: row.get(6)?,
                timestamp: row.get(7)?,
                date: row.get(8)?,
                is_read: row.get(9)?,
                is_flagged: row.get(10)?,
                body_text: row.get(11)?,
                uid_validity: row.get(13)?,
                read_revision: row.get(14)?,
                flag_revision: row.get(15)?,
                ..Default::default()
            };
            messages::append_html_text(&mut message.body_text, row.get_ref(12)?.as_str()?);
            if !visit(message) {
                break;
            }
        }
        Ok(())
    }

    pub fn sender_headers(&self, account: &str, folder: &str) -> Result<Vec<Message>> {
        let mut statement = self.0.prepare(concat!(
            messages::select!(
                "coalesce(b.body_text, ''), '', '[]', coalesce(b.html_text, ''), \
                 coalesce(b.html_tickets, '[]')"
            ),
            " WHERE h.account_id=?1 AND h.folder=?2 ORDER BY h.timestamp DESC, h.uid DESC"
        ))?;
        let rows = statement.query_map(params![account, folder], |row| {
            let mut message = messages::decode(row)?;
            if message.calendar_events.is_empty() {
                message.parcels.clear();
                message.attachments.clear();
            }
            message.body_loaded = false;
            message.inline_media_loaded = false;
            if message.tickets.is_empty() {
                message.tickets = messages::json(row, 30)?;
            }
            messages::append_html_text(&mut message.body_text, row.get_ref(29)?.as_str()?);
            Ok(message)
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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
