mod collect;
mod event_identity;
mod events;
mod parcels;
mod range;
mod subscriptions;

use super::{Message, calendar, parcel, receipt, ticket};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};

pub use collect::entries;
pub use range::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    Subscriptions,
    Parcels,
    Events,
    Tickets,
    Receipts,
    Orders,
}

impl Section {
    pub const ALL: [Self; 6] = [
        Self::Parcels,
        Self::Events,
        Self::Tickets,
        Self::Receipts,
        Self::Orders,
        Self::Subscriptions,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Subscriptions => "Subscriptions",
            Self::Parcels => "Parcels",
            Self::Events => "Events",
            Self::Tickets => "Tickets",
            Self::Receipts => "Receipts",
            Self::Orders => "Orders",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Source {
    pub account: String,
    pub folder: String,
    pub message: Message,
}

#[derive(Clone, Debug)]
pub enum Content {
    Subscription,
    Parcel(parcel::Parcel),
    Event(calendar::Event),
    Ticket(ticket::Reservation),
    Receipt(receipt::Receipt),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub source: Source,
    pub content: Content,
    pub series: Vec<Occurrence>,
}

#[derive(Clone, Debug)]
pub struct Occurrence {
    pub event: calendar::Event,
    pub source: Source,
}

impl Entry {
    pub fn section(&self) -> Section {
        match &self.content {
            Content::Subscription => Section::Subscriptions,
            Content::Parcel(_) => Section::Parcels,
            Content::Event(_) => Section::Events,
            Content::Ticket(_) => Section::Tickets,
            Content::Receipt(receipt) => {
                if receipt.invoice {
                    Section::Receipts
                } else {
                    Section::Orders
                }
            }
        }
    }

    pub fn date(&self) -> Option<&str> {
        match &self.content {
            Content::Subscription => None,
            Content::Parcel(parcel) => {
                let state = parcel.details["deliveryStatus"].as_str();
                let completed = matches!(state, Some("Delivered" | "Returned" | "Cancelled"));
                let status_date = completed
                    .then(|| {
                        parcel
                            .steps
                            .iter()
                            .rev()
                            .find(|step| step.name.as_deref() == state)
                            .and_then(|step| step.date.as_deref())
                    })
                    .flatten();
                status_date
                    .or(parcel.arrival_until.as_deref())
                    .or(parcel.arrival_from.as_deref())
                    .or_else(|| parcel.steps.last().and_then(|step| step.date.as_deref()))
            }
            Content::Event(event) => Some(&event.start),
            Content::Ticket(ticket) => ticket.start.as_deref(),
            Content::Receipt(receipt) => receipt.date.as_deref(),
        }
    }
}

pub fn timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp())
        .or_else(|| {
            ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"]
                .iter()
                .find_map(|format| {
                    NaiveDateTime::parse_from_str(value, format)
                        .ok()
                        .and_then(|date| Local.from_local_datetime(&date).earliest())
                        .map(|date| date.timestamp())
                })
        })
        .or_else(|| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .and_then(|date| Local.from_local_datetime(&date).earliest())
                .map(|date| date.timestamp())
        })
}

pub fn parcel_status(parcel: &parcel::Parcel) -> String {
    let value = &parcel.details["deliveryStatus"];
    let value = value
        .as_array()
        .and_then(|values| values.last())
        .unwrap_or(value);
    let status = value
        .as_str()
        .or_else(|| value["name"].as_str())
        .or_else(|| value["@type"].as_str())
        .or_else(|| parcel.steps.last().and_then(|step| step.name.as_deref()))
        .unwrap_or("In transit");
    match status.rsplit('/').next().unwrap_or(status) {
        "InTransit" => "In transit".into(),
        "OutForDelivery" => "Out for delivery".into(),
        "DeliveryEvent" => "In transit".into(),
        status => status.into(),
    }
}

fn upcoming(value: &str, now: i64) -> bool {
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return DateTime::from_timestamp(now, 0)
            .is_some_and(|now| date >= now.with_timezone(&Local).date_naive());
    }
    timestamp(value).is_some_and(|date| date >= now)
}

fn event_upcoming(event: &calendar::Event, now: i64) -> bool {
    if event.calendar.is_some()
        && let Some(end) = event
            .end
            .as_deref()
            .and_then(|end| NaiveDate::parse_from_str(end, "%Y-%m-%d").ok())
    {
        return DateTime::from_timestamp(now, 0)
            .is_some_and(|now| end > now.with_timezone(&Local).date_naive());
    }
    upcoming(event.end.as_deref().unwrap_or(&event.start), now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(sources: Vec<Source>, now: i64) -> Vec<Entry> {
        super::entries(sources, now, Range::default())
    }

    #[test]
    fn active_undated_parcels_remain_visible_without_including_undated_purchases() {
        let now = timestamp("2026-11-04T12:00:00Z").unwrap();
        let mut undated = source("one", "In transit", 0);
        undated.message.parcels[0].arrival_until = None;
        undated.message.tickets = vec![ticket::Reservation {
            name: "Undated booking".into(),
            ..Default::default()
        }];
        undated.message.body_html = r#"<script type="application/ld+json">{"@type":"Order","orderNumber":"123","price":42,"priceCurrency":"EUR"}</script>"#.into();
        let items = entries(vec![undated], now);
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0].content, Content::Parcel(_)));
    }

    #[test]
    fn overdue_active_parcels_match_their_expected_date_in_the_recent_range() {
        let now = timestamp("2026-11-04T12:00:00Z").unwrap();
        let mut overdue = source("one", "In transit", 0);
        overdue.message.parcels[0].arrival_until = Some("2026-10-20".into());
        assert_eq!(
            super::entries(vec![overdue.clone()], now, Range::Month).len(),
            1
        );
        assert!(super::entries(vec![overdue], now, Range::Week).is_empty());
    }

    fn source(account: &str, status: &str, age: i64) -> Source {
        let html = format!(
            r#"<script type="application/ld+json">{{"@type":"ParcelDelivery","orderNumber":"123","expectedArrivalUntil":"2026-11-05","deliveryStatus":"{status}"}}</script>"#
        );
        Source {
            account: account.into(),
            folder: "INBOX".into(),
            message: Message {
                uid: 1,
                message_id: "parcel".into(),
                sender: "shop@example.com".into(),
                timestamp: timestamp("2026-11-04T12:00:00Z").unwrap() - age,
                parcels: parcel::parse(&super::super::html::Html::new(&html)),
                ..Default::default()
            },
        }
    }

    #[test]
    fn latest_delivery_state_is_preserved_in_history_without_merging_accounts() {
        let now = timestamp("2026-11-04T12:00:00Z").unwrap();
        let sources = vec![
            source("one", "In transit", 30),
            source("one", "https://schema.org/Delivered", 0),
            source("two", "In transit", 10),
        ];
        let items = entries(sources, now);
        assert_eq!(items.len(), 2);
        let delivered = items
            .iter()
            .find(|item| item.source.account == "one")
            .unwrap();
        assert!(
            matches!(&delivered.content, Content::Parcel(parcel) if parcel.details["deliveryStatus"] == "Delivered")
        );
        assert!(items.iter().any(|item| item.source.account == "two"));
        assert_eq!(
            entries(vec![source("one", "In transit", 61 * 86400)], now).len(),
            1
        );
        let mut duplicate = source("two", "In transit", 10);
        duplicate.folder = "All Mail".into();
        assert_eq!(
            entries(vec![source("two", "In transit", 10), duplicate], now).len(),
            1
        );
    }

    #[test]
    fn event_sequence_beats_mail_arrival_order_and_cancellations_hide_older_invites() {
        let now = timestamp("2026-11-04T12:00:00Z").unwrap();
        let mut latest = source("one", "Delivered", 20);
        latest.message.parcels.clear();
        latest.message.calendar_events = vec![calendar::Event {
            uid: Some("meeting".into()),
            name: "Meeting".into(),
            sequence: 2,
            start: "2026-11-05T15:00:00Z".into(),
            ..Default::default()
        }];
        let mut delayed = latest.clone();
        delayed.message.timestamp = now;
        delayed.message.calendar_events[0].sequence = 1;
        delayed.message.calendar_events[0].start = "2026-11-06T15:00:00Z".into();
        let items = entries(vec![delayed.clone(), latest.clone()], now);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].date(), Some("2026-11-05T15:00:00Z"));
        latest.message.calendar_events[0].method = Some("CANCEL".into());
        assert!(entries(vec![delayed, latest], now).is_empty());
    }

    #[test]
    fn repeated_bookings_share_history_across_ids_and_remove_html_copies_of_the_same_date() {
        let now = timestamp("2026-11-04T12:00:00Z").unwrap();
        let booking =
            |account: &str, id: Option<&str>, start: &str, organizer: Option<&str>| Source {
                account: account.into(),
                folder: "INBOX".into(),
                message: Message {
                    message_id: format!("{id:?}-{start}"),
                    sender: "Clinic <bookings@example.com>".into(),
                    timestamp: now,
                    display_prepared: true,
                    calendar_events: vec![calendar::Event {
                        uid: id.map(str::to_owned),
                        organizer: organizer.map(str::to_owned),
                        name: "Psychiatry".into(),
                        start: start.into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            };
        let sources = vec![
            booking(
                "one",
                Some("past"),
                "2026-10-01T10:00:00Z",
                Some("Clinic: Doctor"),
            ),
            booking(
                "one",
                Some("next"),
                "2026-11-05T10:00:00Z",
                Some(" Clinic:  Doctor "),
            ),
            booking("one", None, "2026-11-05T11:00:00+01:00", None),
            booking(
                "two",
                Some("next"),
                "2026-11-05T10:00:00Z",
                Some("Clinic: Doctor"),
            ),
        ];
        let items = super::entries(sources, now, Range::Week);
        assert_eq!(items.len(), 2);
        let series = items
            .iter()
            .find(|entry| entry.source.account == "one")
            .unwrap();
        assert_eq!(series.date(), Some("2026-11-05T10:00:00Z"));
        assert_eq!(series.series.len(), 2);
        assert_eq!(series.series[0].event.uid.as_deref(), Some("past"));
        assert_eq!(series.series[1].event.uid.as_deref(), Some("next"));
        let separate = super::entries(
            vec![
                booking("one", Some("a"), "2026-11-05T10:00:00Z", Some("Doctor A")),
                booking("one", Some("b"), "2026-11-05T10:00:00Z", Some("Doctor B")),
            ],
            now,
            Range::Week,
        );
        assert_eq!(separate.len(), 2);
    }

    #[test]
    fn series_keeps_past_instances_and_latest_revisions_but_selects_next_matching_date() {
        let now = timestamp("2026-11-04T12:00:00Z").unwrap();
        let occurrence = |account: &str, day: &str, sequence: u32| {
            let mut source = source(account, "Delivered", 0);
            source.message.parcels.clear();
            source.message.uid = sequence;
            source.message.calendar_events = vec![calendar::Event {
                uid: Some("weekly".into()),
                organizer_email: Some("host@example.com".into()),
                recurrence_id: Some(format!("RECURRENCE-ID:{day}")),
                name: "Weekly planning".into(),
                start: format!("{day}T15:00:00Z"),
                sequence,
                ..Default::default()
            }];
            source
        };
        let sources = vec![
            occurrence("one", "2026-10-29", 1),
            occurrence("one", "2026-11-05", 1),
            occurrence("one", "2026-11-05", 2),
            occurrence("one", "2026-11-12", 1),
            occurrence("two", "2026-11-05", 1),
        ];
        let items = super::entries(sources.clone(), now, Range::Week);
        assert_eq!(items.len(), 2);
        let series = items
            .iter()
            .find(|entry| entry.source.account == "one")
            .unwrap();
        assert_eq!(series.date(), Some("2026-11-05T15:00:00Z"));
        assert_eq!(series.series.len(), 3);
        assert_eq!(series.series[0].event.start, "2026-10-29T15:00:00Z");
        assert_eq!(series.series[1].event.sequence, 2);
        assert_eq!(series.series[1].source.message.uid, 2);
        let historical = super::entries(
            sources,
            now,
            Range::Custom {
                from: NaiveDate::from_ymd_opt(2026, 10, 29).unwrap(),
                to: NaiveDate::from_ymd_opt(2026, 10, 29).unwrap(),
            },
        );
        assert_eq!(historical.len(), 1);
        assert_eq!(historical[0].date(), Some("2026-10-29T15:00:00Z"));
        assert_eq!(historical[0].id, series.id);
    }

    #[test]
    fn keeps_events_in_progress_and_expires_tickets_after_their_end() {
        let now = timestamp("2026-11-04T12:00:00Z").unwrap();
        let mut source = source("one", "Delivered", 0);
        source.message.parcels.clear();
        source.message.calendar_events = vec![calendar::Event {
            name: "Workshop".into(),
            start: "2026-11-04T10:00:00Z".into(),
            end: Some("2026-11-04T13:00:00Z".into()),
            ..Default::default()
        }];
        source.message.tickets = vec![ticket::Reservation {
            name: "Train".into(),
            start: Some("2026-11-04T09:00:00Z".into()),
            end: Some("2026-11-04T11:00:00Z".into()),
            ..Default::default()
        }];
        let items = entries(vec![source], now);
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0].content, Content::Event(event) if event.name == "Workshop"));
        let mut ended = items[0].source.clone();
        ended.message.calendar_events[0].start = "2026-11-03".into();
        ended.message.calendar_events[0].end = Some("2026-11-04".into());
        ended.message.calendar_events[0].calendar = Some(String::new());
        assert!(entries(vec![ended.clone()], now).is_empty());
        ended.message.calendar_events[0].calendar = None;
        assert_eq!(entries(vec![ended], now).len(), 1);
    }
}
