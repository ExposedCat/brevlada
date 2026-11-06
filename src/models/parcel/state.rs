use super::{Parcel, Step};
use serde_json::Value;

pub fn combine(mut updates: Vec<(i64, Parcel)>) -> Parcel {
    updates.sort_by_key(|(timestamp, _)| *timestamp);
    let mut result = updates[0].1.clone();
    let mut events: Vec<(i64, bool, Step)> = Vec::new();
    let mut current: Option<(bool, i64, u8, String)> = None;
    for (timestamp, mut update) in updates {
        update.hydrate_identity();
        macro_rules! field {
            ($field:ident) => {
                if update.$field.is_some() {
                    result.$field = update.$field.clone();
                }
            };
        }
        field!(item);
        field!(arrival_from);
        field!(arrival_until);
        field!(carrier);
        field!(tracking_url);
        field!(order_url);
        field!(order_number);
        merge(&mut result.details, &update.details);
        let mut steps = update.steps;
        if steps.is_empty() {
            let status = &update.details["deliveryStatus"];
            let values: Vec<_> = status
                .as_array()
                .map(|values| values.iter().collect())
                .unwrap_or_else(|| vec![status]);
            steps = values
                .into_iter()
                .filter_map(|value| {
                    let name = value
                        .as_str()
                        .or_else(|| value["name"].as_str())
                        .or_else(|| value["@type"].as_str())?;
                    (name != "DeliveryEvent").then(|| Step {
                        name: Some(label(name)),
                        date: value["startDate"].as_str().map(str::to_owned),
                        location: None,
                    })
                })
                .collect();
        }
        for mut step in steps {
            let explicit = step.date.is_some();
            let time = step
                .date
                .as_deref()
                .and_then(event_time)
                .unwrap_or(timestamp);
            if let Some(name) = &step.name {
                let name = label(name);
                let rank = terminal(&name);
                let candidate = (rank > 0, time, rank, name.clone());
                if current.as_ref().is_none_or(|current| candidate > *current) {
                    current = Some(candidate);
                }
                step.name = Some(name);
            }
            let duplicate = events.iter_mut().find(|(_, dated, old)| {
                normalized(&old.name) == normalized(&step.name)
                    && normalized(&old.location) == normalized(&step.location)
                    && (old.date == step.date || !*dated || !explicit)
            });
            if let Some(old) = duplicate {
                if explicit && !old.1 {
                    *old = (time, true, step);
                }
            } else {
                events.push((time, explicit, step));
            }
        }
    }
    events.sort_by_key(|(time, _, _)| *time);
    result.steps = events.into_iter().map(|(_, _, step)| step).collect();
    if let Some((_, _, _, state)) = current {
        result.details["deliveryStatus"] = state.into();
    }
    result
}

fn merge(old: &mut Value, new: &Value) {
    if new.is_null() {
        return;
    }
    if let (Some(old), Some(new)) = (old.as_object_mut(), new.as_object()) {
        for (key, value) in new {
            merge(old.entry(key.clone()).or_insert(Value::Null), value);
        }
    } else {
        *old = new.clone();
    }
}

fn normalized(value: &Option<String>) -> Option<String> {
    value.as_ref().map(|value| value.trim().to_lowercase())
}

pub(super) fn label(value: &str) -> String {
    let value = value.rsplit('/').next().unwrap_or(value).trim();
    match value
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "")
        .as_str()
    {
        "delivered" | "parceldelivered" | "packagedelivered" | "deliverycomplete"
        | "deliverycompleted" => "Delivered".into(),
        "intransit" => "In transit".into(),
        "outfordelivery" => "Out for delivery".into(),
        "returned" | "returntosender" | "returnedtosender" => "Returned".into(),
        "cancelled" | "canceled" => "Cancelled".into(),
        _ => value.into(),
    }
}

fn terminal(value: &str) -> u8 {
    match value {
        "Delivered" => 1,
        "Cancelled" => 2,
        "Returned" => 3,
        _ => 0,
    }
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

    #[test]
    fn overdue_requires_a_past_eta_without_confirmed_completion() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-04T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Local);
        let mut parcel: Parcel = serde_json::from_value(serde_json::json!({
            "item": null, "arrival_from": null, "arrival_until": "2026-10-06T21:00:00Z",
            "carrier": null, "tracking_url": null, "order_url": null, "order_number": null,
            "steps": [], "details": {"deliveryStatus": "In transit"}
        }))
        .unwrap();
        assert!(parcel.overdue(now));
        parcel.arrival_until = Some(now.date_naive().to_string());
        assert!(!parcel.overdue(now));
        parcel.arrival_until = Some("2026-11-10".into());
        assert!(!parcel.overdue(now));
        parcel.arrival_until = Some("2026-10-06".into());
        for status in [
            "Delivered",
            "https://schema.org/Delivered",
            "Cancelled",
            "Returned",
        ] {
            parcel.details["deliveryStatus"] = status.into();
            assert!(!parcel.overdue(now));
        }
        parcel.details["deliveryStatus"] = serde_json::Value::Null;
        parcel.steps.push(Step {
            name: Some("Delivered".into()),
            date: None,
            location: None,
        });
        assert!(!parcel.overdue(now));
    }

    #[test]
    fn late_initial_mail_does_not_regress_delivery_and_keeps_order_details() {
        let parse = |value: Value| {
            super::super::parse(&crate::models::html::Html::new(&format!(
                "<script type='application/ld+json'>{value}</script>"
            )))
            .remove(0)
        };
        let ordered = parse(
            serde_json::json!({"@type":"ParcelDelivery","itemShipped":"Shoes","partOfOrder":{"orderNumber":"123","url":"https://shop.example/order/123"},"deliveryStatus":{"name":"Shipped","startDate":"2026-10-01T12:00:00Z"}}),
        );
        let delivered = parse(
            serde_json::json!({"@type":"ParcelDelivery","trackingNumber":"TRACK-1","deliveryStatus":{"name":"Delivered","startDate":"2026-10-03T12:00:00Z"}}),
        );
        let parcel = combine(vec![(300, ordered), (200, delivered)]);
        assert_eq!(parcel.details["deliveryStatus"], "Delivered");
        assert_eq!(parcel.item.as_deref(), Some("Shoes"));
        assert_eq!(parcel.order_number.as_deref(), Some("123"));
        assert_eq!(parcel.details["trackingNumber"], "TRACK-1");
        assert_eq!(parcel.steps[0].name.as_deref(), Some("Shipped"));
        assert_eq!(parcel.steps[1].name.as_deref(), Some("Delivered"));
    }
}
