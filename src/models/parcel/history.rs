use super::{Parcel, Step, parse_message};
use crate::models::{Message, senders};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Identifier {
    Order(String),
    Tracking(String),
}

impl Identifier {
    pub fn value(&self) -> &str {
        match self {
            Self::Order(value) | Self::Tracking(value) => value,
        }
    }
}

pub fn order_id(parcel: &Parcel) -> Option<&str> {
    parcel
        .order_number
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

pub fn identifier(parcel: &Parcel) -> Option<Identifier> {
    if let Some(order) = order_id(parcel) {
        return Some(Identifier::Order(order.to_owned()));
    }
    parcel.details["trackingNumber"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| Identifier::Tracking(value.to_owned()))
}

/// Combine updates with the same sender and explicit order or tracking number.
/// Email timestamps supply dates when an update has no event date of its own.
pub fn sequence<'a>(
    parcel: &Parcel,
    source: &Message,
    messages: impl IntoIterator<Item = &'a Message>,
) -> Parcel {
    let mut result = parcel.clone();
    let Some(id) = identifier(parcel) else {
        return result;
    };
    let sender = senders::key(source);
    if sender.is_empty() {
        return result;
    }
    let mut updates = vec![(source.timestamp, parcel.clone())];
    for message in messages {
        if senders::key(message) != sender {
            continue;
        }
        let parcels = if message.parcels.is_empty() {
            parse_message(message)
        } else {
            message.parcels.clone()
        };
        for related in parcels {
            if identifier(&related).as_ref() == Some(&id) {
                updates.push((message.timestamp, related));
            }
        }
    }
    updates.sort_by_key(|(timestamp, _)| *timestamp);
    let mut events: Vec<(i64, bool, Step)> = Vec::new();
    for (timestamp, update) in updates {
        let mut steps = update.steps;
        if steps.is_empty()
            && let Some(status) = update.details["deliveryStatus"]
                .as_str()
                .filter(|status| !status.trim().is_empty() && !status.contains("://"))
        {
            steps.push(Step {
                name: Some(status.into()),
                date: None,
                location: None,
            });
        }
        for mut step in steps {
            let explicit_date = step.date.is_some();
            let time = step
                .date
                .as_deref()
                .and_then(event_time)
                .unwrap_or(timestamp);
            if step.date.is_none() {
                step.date =
                    chrono::DateTime::from_timestamp(timestamp, 0).map(|date| date.to_rfc3339());
            }
            let duplicate = events.iter_mut().find(|(_, dated, old)| {
                normalized(&old.name) == normalized(&step.name)
                    && normalized(&old.location) == normalized(&step.location)
                    && (old.date == step.date || !*dated || !explicit_date)
            });
            if let Some(existing) = duplicate {
                if explicit_date && !existing.1 {
                    *existing = (time, true, step);
                }
            } else {
                events.push((time, explicit_date, step));
            }
        }
    }
    events.sort_by_key(|(time, _, _)| *time);
    result.steps = events.into_iter().map(|(_, _, step)| step).collect();
    result
}

fn normalized(value: &Option<String>) -> Option<String> {
    value.as_ref().map(|value| value.trim().to_lowercase())
}

fn event_time(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp())
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .map(|date| date.and_utc().timestamp())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(sender: &str, order: Option<&str>, status: &str, timestamp: i64) -> Message {
        Message {
            sender: sender.into(),
            timestamp,
            body_loaded: true,
            body_html: format!(
                "<script type='application/ld+json'>{}</script>",
                json!({
                    "@type": "ParcelDelivery", "partOfOrder": {"orderNumber": order}, "deliveryStatus": status
                })
            ),
            ..Default::default()
        }
    }

    #[test]
    fn combines_ugreen_tracking_updates_without_inventing_an_order_number() {
        let shipped = Message {
            sender: "UGREEN <service.eu@ugreen.com>".into(),
            subject: "Your order KVM Switch has already been shipped.".into(),
            body_html: "<div>Tracking number: CC996635912DE</div><div>Carrier: WINIT Carrier phone: What's inside</div>".into(),
            timestamp: 1789965688, body_loaded: true, ..Default::default()
        };
        let delivered = Message {
            subject: "Your order KVM Switch has been delivered.".into(),
            timestamp: 1790303613,
            ..shipped.clone()
        };
        let other = Message {
            body_html: "<div>Tracking number: CC996635913DE</div>".into(),
            ..delivered.clone()
        };
        let different_sender = Message {
            sender: "other@example.com".into(),
            ..delivered.clone()
        };
        let parcel = parse_message(&delivered).remove(0);
        assert!(parcel.order_number.is_none());
        assert_eq!(
            identifier(&parcel),
            Some(Identifier::Tracking("CC996635912DE".into()))
        );
        let combined = sequence(
            &parcel,
            &delivered,
            [&shipped, &delivered, &other, &different_sender],
        );
        assert!(combined.order_number.is_none());
        assert_eq!(
            combined
                .steps
                .iter()
                .map(|step| step.name.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["In transit", "Delivered"]
        );
        let mut missing = parcel.clone();
        missing.details["trackingNumber"] = json!(" ");
        let candidates = std::iter::from_fn(|| -> Option<&Message> {
            panic!("Must not search without either identifier")
        });
        assert_eq!(sequence(&missing, &delivered, candidates), missing);
    }

    #[test]
    fn order_and_tracking_identifiers_with_the_same_value_are_distinct() {
        let order = message("shop@example.com", Some("123"), "Sent", 100);
        let mut tracking = message("shop@example.com", None, "Delivered", 200);
        tracking.parcels = parse_message(&tracking);
        tracking.parcels[0].details["trackingNumber"] = json!("123");
        let parcel = parse_message(&order).remove(0);
        assert_eq!(sequence(&parcel, &order, [&tracking]).steps.len(), 1);
    }

    #[test]
    fn combines_separate_emails_in_chronological_order_with_email_dates() {
        let sent = message("Shop <shop@example.com>", Some("123"), "Sent", 100);
        let transit = message("SHOP@example.com", Some("123"), "In transit", 200);
        let delivered = message("shop@example.com", Some("123"), "Delivered", 300);
        let parcel = parse_message(&transit).remove(0);
        let result = sequence(&parcel, &transit, [&delivered, &sent, &transit, &sent]);
        assert_eq!(
            result
                .steps
                .iter()
                .map(|step| step.name.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["Sent", "In transit", "Delivered"]
        );
        assert_eq!(
            event_time(result.steps[0].date.as_deref().unwrap()),
            Some(100)
        );
        assert_eq!(result.details, parcel.details);
    }

    #[test]
    fn isolates_senders_orders_and_missing_identifiers() {
        let source = message("shop@example.com", Some("123"), "Sent", 100);
        let other_order = message("shop@example.com", Some("1234"), "Delivered", 200);
        let other_sender = message("other@example.com", Some("123"), "Delivered", 200);
        let missing = message("shop@example.com", None, "Delivered", 200);
        let parcel = parse_message(&source).remove(0);
        assert_eq!(
            sequence(&parcel, &source, [&other_order, &other_sender, &missing])
                .steps
                .len(),
            1
        );
        for id in [None, Some(" ")] {
            let missing = message("shop@example.com", id, "Sent", 100);
            let parcel = parse_message(&missing).remove(0);
            let candidates = std::iter::from_fn(|| -> Option<&Message> {
                panic!("Must not search without an order ID")
            });
            assert_eq!(sequence(&parcel, &missing, candidates), parcel);
        }
    }

    #[test]
    fn respects_event_dates_and_preserves_distinct_locations() {
        let mut source = message("shop@example.com", Some("123"), "Sent", 300);
        source.body_html = format!(
            "<script type='application/ld+json'>{}</script>",
            json!({
                "@type":"ParcelDelivery", "partOfOrder":{"orderNumber":"123"},
                "deliveryStatus":[
                    {"name":"Delivered","startDate":"2026-10-03"},
                    {"name":"In transit","startDate":"2026-10-02", "location":"London"},
                    {"name":"In transit","startDate":"2026-10-01", "location":"Paris"}
                ]
            })
        );
        let parcel = parse_message(&source).remove(0);
        let result = sequence(&parcel, &source, [&source]);
        assert_eq!(result.steps.len(), 3);
        assert_eq!(result.steps[0].location.as_deref(), Some("Paris"));
        assert_eq!(result.steps[2].name.as_deref(), Some("Delivered"));
    }
}
