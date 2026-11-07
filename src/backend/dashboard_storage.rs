use super::{Storage, messages};
use crate::models::dashboard::{self, Entry, Range, Source};
use anyhow::Result;
use rusqlite::params;

impl Storage {
    pub fn dashboard_source(&self, source: &Source) -> Result<Option<crate::models::Message>> {
        Ok(self
            .message(&source.account, &source.folder, source.message.uid)?
            .filter(|message| {
                message.uid_validity == source.message.uid_validity
                    && message.message_id == source.message.message_id
            }))
    }

    pub fn dashboard(&self, accounts: &[String], now: i64, range: Range) -> Result<Vec<Entry>> {
        let mut statement = self.0.prepare(concat!(
            "SELECT ", messages::columns!(),
            ", '', '', '[]', h.account_id,coalesce(h.folder,''),coalesce(i.parcel_data,'[]'),coalesce(i.receipt_data,'[]')
             FROM rust_headers h LEFT JOIN rust_message_items i ON i.header_id=h.id
             WHERE h.id IN (
                SELECT id FROM rust_headers WHERE account_id IN (SELECT value FROM json_each(?1))
                AND folder IS NOT NULL AND NOT is_draft AND NOT is_spam
                AND (parcels<>'[]' OR calendar_events<>'[]' OR tickets<>'[]' OR unsubscribe<>'null')
                UNION SELECT id FROM rust_headers WHERE account_id IN (SELECT value FROM json_each(?1))
                AND folder IS NULL AND NOT is_draft AND calendar_events<>'[]'
                UNION SELECT h.id FROM rust_message_items i JOIN rust_headers h ON h.id=i.header_id
                WHERE h.account_id IN (SELECT value FROM json_each(?1))
                AND h.folder IS NOT NULL AND NOT h.is_draft AND NOT h.is_spam)
             ORDER BY h.timestamp DESC,h.folder<>'INBOX',h.uid DESC"
        ))?;
        let sources = statement
            .query_map(params![serde_json::to_string(accounts)?], |row| {
                let mut message = messages::decode(row)?;
                let parcels: Vec<crate::models::parcel::Parcel> = messages::json(row, 31)?;
                if !parcels.is_empty() {
                    message.parcels = parcels;
                }
                message.receipts = messages::json(row, 32)?;
                message.display_prepared = true;
                Ok(Source {
                    account: row.get(29)?,
                    folder: row.get(30)?,
                    message,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let sources = sources
            .into_iter()
            .filter(|source| !crate::backend::mail_sync::excluded_folder(&source.folder, &[]))
            .collect();
        Ok(dashboard::entries(sources, now, range))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Message, calendar::Event};

    #[test]
    fn subscriptions_aggregate_all_indexed_folders_by_account_and_sender_outside_the_date_range() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let now = dashboard::timestamp("2026-11-07T12:00:00Z").unwrap();
        let message = Message {
            uid: 1,
            message_id: "newsletter-old".into(),
            sender: "News <news@example.com>".into(),
            timestamp: now - 400 * 86400,
            unsubscribe: Some(crate::models::Unsubscribe {
                url: "https://example.com/unsubscribe/old".into(),
                one_click: Some(true),
            }),
            ..Default::default()
        };
        for (account, folder) in [
            ("one", "INBOX"),
            ("one", "Archive"),
            ("two", "Newsletters"),
            ("other", "INBOX"),
        ] {
            storage.store(account, folder, &message).unwrap();
        }
        let latest = Message {
            uid: 2,
            message_id: "newsletter-latest".into(),
            timestamp: now - 200 * 86400,
            unsubscribe: Some(crate::models::Unsubscribe {
                url: "https://example.com/unsubscribe/latest".into(),
                one_click: Some(true),
            }),
            ..message.clone()
        };
        storage.store("one", "Archive", &latest).unwrap();
        storage
            .store(
                "one",
                "INBOX",
                &Message {
                    uid: 3,
                    message_id: "not-a-subscription".into(),
                    timestamp: now,
                    unsubscribe: None,
                    ..message.clone()
                },
            )
            .unwrap();
        for folder in ["Trash", "Junk", "Drafts"] {
            storage
                .store(
                    "one",
                    folder,
                    &Message {
                        sender: format!("{folder}@example.com"),
                        is_draft: folder == "Drafts",
                        ..message.clone()
                    },
                )
                .unwrap();
        }
        let items = storage
            .dashboard(&["one".into(), "two".into()], now, Range::Week)
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .all(|entry| matches!(entry.content, dashboard::Content::Subscription))
        );
        let one = items
            .iter()
            .find(|entry| entry.source.account == "one")
            .unwrap();
        assert_eq!(one.source.folder, "Archive");
        assert_eq!(one.source.message.uid, 2);
        assert_eq!(
            one.source.message.unsubscribe.as_ref().unwrap().url,
            "https://example.com/unsubscribe/latest"
        );
        assert!(
            !storage
                .message("one", "Archive", 2)
                .unwrap()
                .unwrap()
                .is_read
        );
        assert_eq!(
            storage
                .dashboard(&["two".into()], now, Range::Year)
                .unwrap()
                .len(),
            1
        );
        assert!(storage.dashboard(&[], now, Range::Week).unwrap().is_empty());
    }

    #[test]
    fn dashboard_events_include_local_rsvp_state() {
        use crate::models::calendar::Attendee;
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let now = dashboard::timestamp("2026-11-04T12:00:00Z").unwrap();
        let event = Event {
            uid: Some("invite".into()),
            name: "Meeting".into(),
            start: "2026-11-06T12:00:00Z".into(),
            organizer_email: Some("host@example.com".into()),
            method: Some("REQUEST".into()),
            sequence: 1,
            stamp: 1,
            attendees: vec![Attendee {
                email: "one".into(),
                name: None,
                status: "NEEDS-ACTION".into(),
            }],
            ..Default::default()
        };
        storage
            .store(
                "one",
                "INBOX",
                &Message {
                    uid: 1,
                    sender: "host@example.com".into(),
                    calendar_events: vec![event.clone()],
                    ..Default::default()
                },
            )
            .unwrap();
        let mut reply = event;
        reply.method = Some("REPLY".into());
        reply.stamp = 2;
        reply.attendees[0].status = "ACCEPTED".into();
        storage
            .store_sent(
                "one",
                &Message {
                    sender: "one".into(),
                    message_id: "reply".into(),
                    calendar_events: vec![reply],
                    ..Default::default()
                },
            )
            .unwrap();
        let items = storage
            .dashboard(&["one".into()], now, Range::Week)
            .unwrap();
        assert_eq!(items.len(), 1);
        let dashboard::Content::Event(event) = &items[0].content else {
            panic!()
        };
        assert_eq!(event.attendees[0].status, "ACCEPTED");
        assert_eq!(items[0].source.folder, "INBOX");
    }

    #[test]
    #[ignore = "Profiles a copied cache specified by BREVLADA_DASHBOARD_DB without starting the application"]
    fn profile_dashboard_load() {
        let path = std::path::PathBuf::from(std::env::var("BREVLADA_DASHBOARD_DB").unwrap());
        let start = std::time::Instant::now();
        let storage = Storage::open(&path).unwrap();
        eprintln!("dashboard open: {:?}", start.elapsed());
        let accounts: Vec<String> = storage
            .0
            .prepare("SELECT DISTINCT account_id FROM rust_headers")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for range in [Range::Month, Range::Week, Range::Year, Range::Month] {
            let start = std::time::Instant::now();
            let items = storage
                .dashboard(&accounts, chrono::Utc::now().timestamp(), range)
                .unwrap();
            eprintln!(
                "dashboard {}: {:?}, {} cards, {} series, largest history {} occurrences",
                range.label(),
                start.elapsed(),
                items.len(),
                items
                    .iter()
                    .filter(|entry| !entry.series.is_empty())
                    .count(),
                items
                    .iter()
                    .map(|entry| entry.series.len())
                    .max()
                    .unwrap_or(0)
            );
        }
    }

    #[test]
    fn ranges_use_purchase_and_event_dates_instead_of_mail_dates() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let now = dashboard::timestamp("2026-11-04T12:00:00Z").unwrap();
        for (uid, order, date, age) in [
            (1, "recent", "2026-11-03", 400),
            (2, "older", "2026-08-01", 0),
        ] {
            storage.store("one", "INBOX", &Message {
                uid, message_id: order.into(), sender: "shop@example.com".into(), timestamp: now - age * 86400,
                body_loaded: true,
                body_html: format!(r#"<script type="application/ld+json">{{"@type":"Order","orderNumber":"{order}","orderDate":"{date}","price":42,"priceCurrency":"EUR"}}</script>"#),
                ..Default::default()
            }).unwrap();
        }
        let items = storage
            .dashboard(&["one".into()], now, Range::Week)
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source.message.message_id, "recent");
        assert_eq!(
            storage
                .dashboard(&["one".into()], now, Range::Year)
                .unwrap()
                .len(),
            2
        );
        for (uid, name, start) in [(3, "soon", "2026-11-06"), (4, "later", "2026-11-25")] {
            storage
                .store(
                    "one",
                    "INBOX",
                    &Message {
                        uid,
                        message_id: name.into(),
                        timestamp: now - 400 * 86400,
                        calendar_events: vec![Event {
                            name: name.into(),
                            start: start.into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let items = storage
            .dashboard(&["one".into()], now, Range::Week)
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .any(|entry| entry.source.message.message_id == "soon")
        );
        assert!(
            !items
                .iter()
                .any(|entry| entry.source.message.message_id == "later")
        );
        assert_eq!(
            storage
                .dashboard(&["one".into()], now, Range::Month)
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn dashboard_reads_all_selected_folders_without_marking_mail_read() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let now = dashboard::timestamp("2026-11-04T12:00:00Z").unwrap();
        let message = Message {
            uid: 1,
            message_id: "invitation".into(),
            sender: "host@example.com".into(),
            timestamp: now,
            calendar_events: vec![Event {
                uid: Some("event".into()),
                name: "Meeting".into(),
                start: "2026-11-05T12:00:00Z".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        for (account, folder) in [
            ("one", "INBOX"),
            ("one", "Archive"),
            ("two", "Work"),
            ("three", "INBOX"),
        ] {
            storage.store(account, folder, &message).unwrap();
        }
        let items = storage
            .dashboard(&["one".into(), "two".into()], now, Range::default())
            .unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            items
                .iter()
                .any(|entry| entry.source.account == "two" && entry.source.folder == "Work")
        );
        assert!(!storage.message("one", "INBOX", 1).unwrap().unwrap().is_read);
        let source = &items
            .iter()
            .find(|entry| entry.source.account == "two")
            .unwrap()
            .source;
        assert!(storage.dashboard_source(source).unwrap().is_some());
        let stale = Source {
            message: Message {
                uid_validity: Some(99),
                ..source.message.clone()
            },
            ..source.clone()
        };
        assert!(storage.dashboard_source(&stale).unwrap().is_none());
        let stale = Source {
            message: Message {
                message_id: "different-message".into(),
                ..source.message.clone()
            },
            ..source.clone()
        };
        assert!(storage.dashboard_source(&stale).unwrap().is_none());
        assert!(
            storage
                .dashboard(&[], now, Range::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn receipts_expire_and_trash_spam_and_drafts_are_excluded() {
        let storage = Storage::open(std::path::Path::new(":memory:")).unwrap();
        let now = dashboard::timestamp("2026-11-04T12:00:00Z").unwrap();
        let receipt = Message {
            uid: 1, message_id: "receipt".into(), sender: "shop@example.com".into(), timestamp: now,
            body_loaded: true,
            body_html: r#"<script type="application/ld+json">{"@type":"Order","orderNumber":"123","orderDate":"2026-11-04","price":42,"priceCurrency":"EUR"}</script>"#.into(),
            ..Default::default()
        };
        for folder in ["[Gmail]/Trash", "INBOX.Spam.Old"] {
            storage.store("one", folder, &receipt).unwrap();
        }
        storage
            .store(
                "one",
                "Drafts",
                &Message {
                    is_draft: true,
                    ..receipt.clone()
                },
            )
            .unwrap();
        storage
            .store(
                "one",
                "Junk",
                &Message {
                    is_spam: true,
                    ..receipt.clone()
                },
            )
            .unwrap();
        assert!(
            storage
                .dashboard(&["one".into()], now, Range::default())
                .unwrap()
                .is_empty()
        );
        storage.store("one", "Purchases", &receipt).unwrap();
        let items = storage
            .dashboard(&["one".into()], now, Range::default())
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source.folder, "Purchases");
        assert!(items[0].source.message.body_html.is_empty());
        assert!(
            storage
                .dashboard(&["one".into()], now + 31 * 86400, Range::default())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            storage
                .dashboard(&["one".into()], now + 31 * 86400, Range::Year)
                .unwrap()
                .len(),
            1
        );
        let from = chrono::DateTime::from_timestamp(now, 0)
            .unwrap()
            .with_timezone(&chrono::Local)
            .date_naive();
        assert_eq!(
            storage
                .dashboard(
                    &["one".into()],
                    now + 31 * 86400,
                    Range::Custom { from, to: from }
                )
                .unwrap()
                .len(),
            1
        );
        let next_day = from.succ_opt().unwrap();
        assert!(
            storage
                .dashboard(
                    &["one".into()],
                    now + 31 * 86400,
                    Range::Custom {
                        from: next_day,
                        to: next_day
                    }
                )
                .unwrap()
                .is_empty()
        );
    }
}
