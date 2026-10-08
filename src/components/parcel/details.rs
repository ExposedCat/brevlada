use serde_json::Value;

pub(super) struct Section {
    pub title: String,
    pub rows: Vec<Detail>,
}

pub(super) struct Detail {
    pub title: String,
    pub value: String,
    pub url: Option<String>,
}

pub(super) fn address(value: &Value) -> Option<String> {
    fn text(value: &Value) -> Option<String> {
        let text = value
            .as_str()
            .or_else(|| value.get("name").and_then(Value::as_str))?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        (!text.is_empty()).then_some(text)
    }

    if value.is_string() {
        return text(value);
    }
    let parts: Vec<_> = [
        "streetAddress",
        "addressLocality",
        "addressRegion",
        "postalCode",
        "addressCountry",
    ]
    .into_iter()
    .filter_map(|key| value.get(key).and_then(text))
    .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

pub(super) fn date(value: &str) -> String {
    date_in_timezone(value, &chrono::Local)
}

fn date_in_timezone<T: chrono::TimeZone>(value: &str, timezone: &T) -> String
where
    T::Offset: std::fmt::Display,
{
    if let Ok(date) = chrono::DateTime::parse_from_rfc3339(value) {
        return date
            .with_timezone(timezone)
            .format("%-d %b %Y, %H:%M")
            .to_string();
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|date| date.format("%-d %b %Y").to_string())
        .unwrap_or_else(|_| value.to_owned())
}

fn words(value: &str) -> String {
    let chars: Vec<_> = value.chars().collect();
    let mut words = String::new();
    for (index, &character) in chars.iter().enumerate() {
        if character == '_' || character == '-' || character.is_whitespace() {
            if !words.is_empty() && !words.ends_with(' ') {
                words.push(' ');
            }
            continue;
        }
        if character.is_uppercase()
            && index > 0
            && (chars[index - 1].is_lowercase()
                || (chars[index - 1].is_uppercase()
                    && chars.get(index + 1).is_some_and(|next| next.is_lowercase())))
            && !words.ends_with(' ')
        {
            words.push(' ');
        }
        words.push(character);
    }
    let words = words
        .split_whitespace()
        .map(|word| {
            if word.chars().all(|character| !character.is_lowercase()) {
                word.to_owned()
            } else {
                word.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn field_label(key: &str) -> String {
    match key {
        "itemShipped" => "Item",
        "partOfOrder" => "Order",
        "deliveryStatus" => "Status",
        "expectedArrivalFrom" => "Arriving from",
        "expectedArrivalUntil" => "Arriving by",
        "expectedArrival" => "Expected delivery",
        "deliveryAddress" => "Delivery address",
        "shippingAddress" => "Shipping address",
        "billingAddress" => "Billing address",
        "addressLocality" => "City",
        "addressRegion" => "Region",
        "addressCountry" => "Country",
        "streetAddress" => "Street address",
        "postalCode" => "Postal code",
        "orderDate" => "Ordered on",
        "orderStatus" => "Order status",
        "startDate" | "availableFrom" => "Date",
        "endDate" | "availableThrough" => "Until",
        "trackingUrl" | "trackingURL" => "Tracking",
        "url" | "URL" | "urlTemplate" => "Website",
        "potentialAction" => "Tracking",
        "target" => "Link",
        "sku" => "SKU",
        "gtin" => "Barcode",
        "priceCurrency" => "Currency",
        "telephone" => "Phone",
        _ => return words(key),
    }
    .into()
}

fn detail(key: &str, value: &Value, section: &str) -> Option<Detail> {
    let text = match value {
        Value::String(value) => value.trim().to_owned(),
        Value::Bool(value) => if *value { "Yes" } else { "No" }.into(),
        Value::Number(value) => value.to_string(),
        _ => return None,
    };
    if text.is_empty() {
        return None;
    }
    let mut row = Detail {
        title: field_label(key),
        value: date(&text),
        url: None,
    };
    // Schema enum URLs describe a status; they aren't links to a user's order.
    if let Some(term) = ["https://schema.org/", "http://schema.org/"]
        .iter()
        .find_map(|prefix| text.strip_prefix(prefix))
    {
        row.value = words(term.strip_prefix("Order").unwrap_or(term));
    } else if text.starts_with("https://") || text.starts_with("http://") {
        // EntryPoint templates with placeholders aren't usable destinations.
        if text.contains(['{', '}']) {
            return None;
        }
        row.value = match key {
            "image" => "View image",
            "trackingUrl" | "trackingURL" => "Track package",
            _ if section == "Tracking" || section.starts_with("Tracking ") => "Track package",
            _ if section == "Order" || section.starts_with("Order ") => "View order",
            _ if section == "Item" || section.starts_with("Item ") => "View item",
            _ => "Open website",
        }
        .into();
        row.url = Some(text);
    }
    Some(row)
}

pub(super) fn sections(value: &Value) -> Vec<Section> {
    let mut output = Vec::new();
    collect(value, "Delivery", &mut output);
    output
}

fn collect(value: &Value, title: &str, output: &mut Vec<Section>) {
    match value {
        Value::Object(map) => {
            let mut section = Section {
                title: title.into(),
                rows: Vec::new(),
            };
            let mut children = Vec::new();
            let mut fields: Vec<_> = map
                .iter()
                .filter(|(key, _)| !key.starts_with('@'))
                .collect();
            fields.sort_by_key(|(key, _)| match key.as_str() {
                "name" => 0,
                "orderNumber" | "trackingNumber" => 1,
                "deliveryStatus" | "orderStatus" => 2,
                "orderDate" | "expectedArrivalFrom" | "startDate" => 3,
                "expectedArrivalUntil" | "expectedArrival" | "endDate" => 4,
                "partOfOrder" => 6,
                "itemShipped" => 7,
                "deliveryAddress" => 8,
                "potentialAction" => 9,
                _ => 5,
            });
            for (key, value) in fields {
                // A carrier or location often wraps a single name in an object.
                let named_value = value.as_object().and_then(|object| {
                    (object.keys().filter(|key| !key.starts_with('@')).count() == 1)
                        .then(|| object.get("name"))
                        .flatten()
                });
                if let Some(row) = detail(key, named_value.unwrap_or(value), title) {
                    section.rows.push(row);
                } else if value.is_object() || value.is_array() {
                    children.push((key, value));
                }
            }
            if !section.rows.is_empty() {
                output.push(section);
            }
            for (key, child) in children {
                let heading = if key == "target" {
                    title.to_owned()
                } else if key == "deliveryStatus" {
                    "Delivery update".into()
                } else {
                    field_label(key)
                };
                collect(child, &heading, output);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let heading = if items.len() == 1 {
                    title.to_owned()
                } else {
                    format!("{title} {}", index + 1)
                };
                if item.is_object() || item.is_array() {
                    collect(item, &heading, output);
                } else if let Some(row) = detail(title, item, title) {
                    output.push(Section {
                        title: heading,
                        rows: vec![row],
                    });
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dates_use_local_time_without_shifting_calendar_only_dates() {
        let timezone = chrono::FixedOffset::east_opt(3 * 3600).unwrap();
        assert_eq!(
            date_in_timezone("2026-09-30T23:30:00Z", &timezone),
            "1 Oct 2026, 02:30"
        );
        assert_eq!(date_in_timezone("2026-09-30", &timezone), "30 Sep 2026");
        assert_eq!(date_in_timezone("Tomorrow", &timezone), "Tomorrow");
    }

    #[test]
    fn groups_order_items_address_and_updates_with_readable_values() {
        let sections = sections(&json!({
            "@type": "ParcelDelivery",
            "@context": "https://schema.org",
            "carrier": {"@type": "Organization", "name": "Parcel Co"},
            "trackingUrl": "https://example.com/track?token=abc",
            "partOfOrder": {
                "orderNumber": "123",
                "orderStatus": "https://schema.org/OrderInTransit",
                "url": "https://example.com/orders/123"
            },
            "itemShipped": [{"name": "Shoes", "sku": "SH-1"}],
            "deliveryAddress": {"streetAddress": "1 Main St\nApt 2", "addressLocality": "London"},
            "deliveryStatus": [{"name": "Shipped", "startDate": "2026-09-30"}],
            "empty": "", "missing": null
        }));
        let delivery = &sections[0];
        assert_eq!(delivery.title, "Delivery");
        assert_eq!(delivery.rows[0].value, "Parcel Co");
        assert_eq!(delivery.rows[1].value, "Track package");
        assert_eq!(
            delivery.rows[1].url.as_deref(),
            Some("https://example.com/track?token=abc")
        );
        let order = sections
            .iter()
            .find(|section| section.title == "Order")
            .unwrap();
        assert_eq!(order.rows[0].title, "Order number");
        assert_eq!(order.rows[1].value, "In transit");
        assert_eq!(order.rows[1].url, None);
        assert_eq!(order.rows[2].value, "View order");
        assert!(sections.iter().any(|section| section.title == "Item"));
        assert!(
            sections
                .iter()
                .any(|section| section.title == "Delivery address")
        );
        assert!(
            sections
                .iter()
                .any(|section| section.title == "Delivery update")
        );
        assert!(
            sections
                .iter()
                .flat_map(|section| &section.rows)
                .all(|row| {
                    !row.title.contains('·')
                        && !row.title.starts_with('@')
                        && !row.value.contains("://")
                })
        );
    }

    #[test]
    fn handles_acronyms_custom_fields_and_missing_details() {
        assert_eq!(field_label("trackingURL"), "Tracking");
        assert_eq!(field_label("custom_fieldName"), "Custom field name");
        assert_eq!(field_label("URLValue"), "URL value");
        assert!(sections(&json!({"@type": "ParcelDelivery", "name": "", "empty": []})).is_empty());
        let rows = sections(&json!({"requiresSignature": true, "quantity": 2}));
        assert!(rows[0].rows.iter().any(|row| row.value == "Yes"));
        assert!(rows[0].rows.iter().any(|row| row.value == "2"));
    }

    #[test]
    fn tracking_action_links_have_a_name_and_templates_are_omitted() {
        let groups = sections(&json!({"potentialAction": {
            "@type": "TrackAction", "target": {"urlTemplate": "https://example.com/track"}
        }}));
        assert_eq!(groups[0].rows[0].value, "Track package");
        assert!(
            sections(&json!({"potentialAction": {
                "target": {"urlTemplate": "https://example.com/{trackingNumber}"}
            }}))
            .is_empty()
        );
    }
}
