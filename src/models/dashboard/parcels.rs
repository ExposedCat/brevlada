use super::*;
use crate::models::senders;
use std::collections::{BTreeMap, HashMap, HashSet};

type Update<'a> = (&'a Source, &'a parcel::Parcel, usize);

pub(super) fn entries(sources: &[Source], now: i64, range: Range) -> Vec<Entry> {
    let updates: Vec<Update<'_>> = sources
        .iter()
        .flat_map(|source| {
            source
                .message
                .parcels
                .iter()
                .enumerate()
                .map(move |(index, parcel)| (source, parcel, index))
        })
        .collect();
    let mut orders: HashMap<String, HashSet<String>> = HashMap::new();
    for (source, parcel, _) in &updates {
        if let (Some(order), Some(tracking)) =
            (order_key(source, parcel), tracking_key(source, parcel))
        {
            orders.entry(order).or_default().insert(tracking);
        }
    }
    let mut groups: BTreeMap<String, Vec<Update<'_>>> = BTreeMap::new();
    for update in updates {
        let (source, parcel, index) = update;
        let order = order_key(source, parcel);
        let tracking = tracking_key(source, parcel);
        let key = if let Some(tracking) = tracking {
            tracking
        } else if let Some(order) = order {
            let tracking = orders.get(&order);
            if let Some(tracking) = tracking.filter(|tracking| tracking.len() > 1) {
                for tracking in tracking {
                    groups.entry(tracking.clone()).or_default().push(update);
                }
                continue;
            }
            tracking
                .and_then(|tracking| tracking.iter().next())
                .cloned()
                .unwrap_or(order)
        } else {
            serde_json::to_string(&(
                &source.account,
                "message",
                &source.message.message_id,
                source.message.message_id.is_empty().then_some((
                    &source.folder,
                    source.message.uid_validity,
                    source.message.uid,
                )),
                index,
            ))
            .unwrap()
        };
        groups.entry(key).or_default().push(update);
    }
    let past = range.past(now);
    let future = range.future(now);
    groups
        .into_iter()
        .filter_map(|(id, updates)| {
            let combined = parcel::combine(
                updates
                    .iter()
                    .map(|(source, parcel, _)| (source.message.timestamp, (*parcel).clone()))
                    .collect(),
            );
            let current = parcel_status(&combined);
            let latest = updates
                .iter()
                .max_by_key(|(source, _, _)| source.message.timestamp)
                .unwrap()
                .0;
            let source = updates
                .iter()
                .filter(|(_, parcel, _)| {
                    parcel_status(parcel) == current
                        || parcel.steps.iter().any(|step| {
                            step.name
                                .as_deref()
                                .is_some_and(|name| name.eq_ignore_ascii_case(&current))
                        })
                })
                .max_by_key(|(source, _, _)| source.message.timestamp)
                .map(|(source, _, _)| *source)
                .unwrap_or(latest);
            let dated = combined
                .arrival_from
                .as_deref()
                .or(combined.arrival_until.as_deref());
            let completed = matches!(current.as_str(), "Delivered" | "Returned" | "Cancelled");
            let mut explicit_dates = updates
                .iter()
                .flat_map(|(_, parcel, _)| parcel.steps.iter())
                .filter_map(|step| step.date.as_deref());
            let recent_update = updates.iter().any(|(source, parcel, _)| {
                (dated.is_none() || (completed && parcel_status(parcel) == current))
                    && (past.from..past.until).contains(&source.message.timestamp)
            });
            let matches = dated.is_some_and(|start| {
                past.overlaps(start, combined.arrival_until.as_deref(), false)
                    || future.overlaps(start, combined.arrival_until.as_deref(), false)
            }) || explicit_dates
                .any(|date| past.overlaps(date, None, false) || future.overlaps(date, None, false))
                || recent_update
                || (dated.is_none()
                    && combined.steps.is_empty()
                    && latest.message.timestamp >= now - 60 * 86400);
            matches.then(|| Entry {
                id,
                source: Source {
                    account: source.account.clone(),
                    folder: source.folder.clone(),
                    message: source.message.list_header(false),
                },
                content: Content::Parcel(combined),
                series: Vec::new(),
            })
        })
        .collect()
}

fn tracking_key(source: &Source, parcel: &parcel::Parcel) -> Option<String> {
    let number = parcel.details["trackingNumber"]
        .as_str()
        .map(str::trim)
        .filter(|number| !number.is_empty());
    number
        .map(|number| {
            serde_json::to_string(&(&source.account, "tracking", number.to_ascii_uppercase()))
                .unwrap()
        })
        .or_else(|| {
            parcel
                .tracking_url
                .as_deref()
                .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
                .map(|url| serde_json::to_string(&(&source.account, "tracking-url", url)).unwrap())
        })
}

fn order_key(source: &Source, parcel: &parcel::Parcel) -> Option<String> {
    let order = parcel.order_id()?;
    (!order.is_empty()).then(|| {
        serde_json::to_string(&(
            &source.account,
            "order",
            senders::key(&source.message),
            order,
        ))
        .unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(
        account: &str,
        sender: &str,
        order: Option<&str>,
        tracking: Option<&str>,
        status: &str,
        date: &str,
    ) -> Source {
        let html = format!(
            "<script type='application/ld+json'>{}</script>",
            serde_json::json!({"@type":"ParcelDelivery","orderNumber":order,"trackingNumber":tracking,"deliveryStatus":status,"itemShipped":"Package"})
        );
        Source {
            account: account.into(),
            folder: "INBOX".into(),
            message: Message {
                sender: sender.into(),
                timestamp: timestamp(date).unwrap(),
                message_id: date.into(),
                parcels: parcel::parse(&super::super::super::html::Html::new(&html)),
                ..Default::default()
            },
        }
    }

    #[test]
    fn alza_provider_links_merge_old_cached_updates_with_different_signed_tokens() {
        let cached = |token: &str, carrier: &str| {
            let mut source = update(
                "one",
                "shop@alza.cz",
                None,
                None,
                "DeliveryEvent",
                "2026-11-01T10:00:00Z",
            );
            source.message.message_id = token.into();
            let parcel = &mut source.message.parcels[0];
            parcel.steps.clear();
            parcel.details = serde_json::json!({"provider": {
                "name": carrier, "url": format!("https://www.alza.cz/my-account/objednavka-123.htm?x={token}")
            }});
            parcel.arrival_until = Some("2026-11-05".into());
            source
        };
        let items = entries(
            &[cached("first", "AlzaBox"), cached("second", "DPD")],
            timestamp("2026-11-04T12:00:00Z").unwrap(),
            Range::Month,
        );
        assert_eq!(items.len(), 1);
        let Content::Parcel(parcel) = &items[0].content else {
            panic!()
        };
        assert_eq!(parcel.order_number.as_deref(), Some("123"));
        assert!(parcel.order_url.is_some());
        assert_eq!(parcel.timeline().len(), 2);
        assert!(parcel.timeline().iter().all(|point| !point.confirmed));
    }

    #[test]
    fn historical_shipments_use_the_current_state_from_all_linked_updates() {
        let first = update(
            "one",
            "shop@example.com",
            Some("123"),
            None,
            "Ordered",
            "2026-10-01T10:00:00Z",
        );
        let shipped = update(
            "one",
            "shop@example.com",
            Some("123"),
            Some("TRACK-1"),
            "In transit",
            "2026-10-03T10:00:00Z",
        );
        let delivered = update(
            "one",
            "carrier@example.com",
            None,
            Some("TRACK-1"),
            "Delivered",
            "2026-11-01T10:00:00Z",
        );
        let mut copy = shipped.clone();
        copy.folder = "All Mail".into();
        let range = Range::Custom {
            from: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            to: chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap(),
        };
        let items = entries(
            &[first, shipped, copy, delivered.clone()],
            timestamp("2026-11-04T12:00:00Z").unwrap(),
            range,
        );
        assert_eq!(items.len(), 1);
        let Content::Parcel(parcel) = &items[0].content else {
            panic!()
        };
        assert_eq!(parcel.details["deliveryStatus"], "Delivered");
        assert_eq!(parcel.order_number.as_deref(), Some("123"));
        assert_eq!(parcel.steps.len(), 3);
        assert_eq!(
            items[0].source.message.message_id,
            delivered.message.message_id
        );
    }

    #[test]
    fn separate_tracking_numbers_in_one_order_are_separate_shipments() {
        let first = update(
            "one",
            "shop@example.com",
            Some("123"),
            Some("A"),
            "Shipped",
            "2026-11-01T10:00:00Z",
        );
        let second = update(
            "one",
            "shop@example.com",
            Some("123"),
            Some("B"),
            "Shipped",
            "2026-11-01T10:00:00Z",
        );
        let generic = update(
            "one",
            "shop@example.com",
            Some("123"),
            None,
            "Ordered",
            "2026-11-01T10:00:00Z",
        );
        let other = update(
            "two",
            "shop@example.com",
            Some("123"),
            Some("A"),
            "Shipped",
            "2026-11-01T10:00:00Z",
        );
        assert_eq!(
            entries(
                &[first, second, generic, other],
                timestamp("2026-11-04T12:00:00Z").unwrap(),
                Range::Week
            )
            .len(),
            3
        );
    }
}
