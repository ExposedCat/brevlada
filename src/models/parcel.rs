use super::{Message, schema::microdata};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

mod history;
pub use history::{Identifier, identifier, sequence};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Parcel {
    pub item: Option<String>,
    pub arrival_from: Option<String>,
    pub arrival_until: Option<String>,
    pub carrier: Option<String>,
    pub tracking_url: Option<String>,
    pub order_url: Option<String>,
    pub order_number: Option<String>,
    pub steps: Vec<Step>,
    pub details: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub name: Option<String>,
    pub date: Option<String>,
    pub location: Option<String>,
}

pub fn parse(html: &str) -> Vec<Parcel> {
    let document = Html::parse_fragment(html);
    let selector = Selector::parse("script[type='application/ld+json']").unwrap();
    let mut parcels = Vec::new();
    for script in document.select(&selector) {
        let source = script.text().collect::<String>();
        if let Ok(value) = serde_json::from_str::<Value>(&source) {
            collect(&value, &mut parcels);
        }
    }
    let selector = Selector::parse("[itemscope][itemtype]").unwrap();
    for element in document.select(&selector) {
        if element
            .value()
            .attr("itemtype")
            .is_some_and(|kind| kind.ends_with("ParcelDelivery"))
        {
            parcels.push(from_value(&microdata(&element)));
        }
    }
    parcels
}

pub fn parse_message(message: &Message) -> Vec<Parcel> {
    let ugreen = super::senders::key(message) == "service.eu@ugreen.com";
    if !ugreen && !message.body_html.contains("ParcelDelivery") {
        return Vec::new();
    }
    let structured = if message.body_html.contains("ParcelDelivery") {
        parse(&message.body_html)
    } else {
        Vec::new()
    };
    if !structured.is_empty() {
        return structured;
    }
    if !ugreen {
        return Vec::new();
    }
    let document = Html::parse_fragment(&message.body_html);
    let text = document.root_element().text().collect::<Vec<_>>().join(" ");
    static TRACKING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)Tracking number:\s*([A-Za-z0-9-]{6,40})").unwrap()
    });
    static CARRIER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)Carrier:\s*([A-Za-z0-9 -]{2,40}?)\s+(?:Carrier phone|What's inside)",
        )
        .unwrap()
    });
    static ITEM: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^Your order (.+?) has (?:already been shipped|been delivered)")
            .unwrap()
    });
    let Some(tracking_number) = TRACKING
        .captures(&text)
        .map(|capture| capture[1].to_owned())
    else {
        return Vec::new();
    };
    let carrier = CARRIER
        .captures(&text)
        .map(|capture| capture[1].trim().to_owned());
    let item = ITEM
        .captures(&message.subject)
        .map(|capture| capture[1].to_owned());
    let selector = Selector::parse("a[href]").unwrap();
    let tracking_url = document.select(&selector).find_map(|link| {
        let title = link.text().collect::<String>();
        if title.to_ascii_lowercase().contains("track your order") {
            link.value()
                .attr("href")
                .filter(|url| url.starts_with("https://"))
                .map(str::to_owned)
        } else {
            None
        }
    });
    let status = if message.subject.to_ascii_lowercase().contains("delivered") {
        "Delivered"
    } else {
        "In transit"
    };
    static ORDER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\bOrder\s*(?:number|id|#)\s*:?\s*#?\s*([A-Za-z0-9][A-Za-z0-9-]*)")
            .unwrap()
    });
    let order_number = ORDER
        .captures(&text)
        .or_else(|| ORDER.captures(&message.subject))
        .map(|capture| capture[1].to_owned());
    let mut details = Map::new();
    if let Some(number) = &order_number {
        details.insert(
            "partOfOrder".into(),
            serde_json::json!({"orderNumber": number}),
        );
    }
    details.insert("trackingNumber".into(), tracking_number.into());
    if let Some(item) = &item {
        details.insert("itemShipped".into(), item.clone().into());
    }
    if let Some(carrier) = &carrier {
        details.insert("carrier".into(), carrier.clone().into());
    }
    if let Some(url) = &tracking_url {
        details.insert("trackingUrl".into(), url.clone().into());
    }
    details.insert("deliveryStatus".into(), status.into());
    vec![Parcel {
        item,
        arrival_from: None,
        arrival_until: None,
        carrier,
        tracking_url,
        order_url: None,
        order_number,
        steps: Vec::new(),
        details: Value::Object(details),
    }]
}

fn collect(value: &Value, parcels: &mut Vec<Parcel>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect(item, parcels)),
        Value::Object(map) => {
            let is_parcel = map.get("@type").is_some_and(|kind| match kind {
                Value::String(kind) => kind.ends_with("ParcelDelivery"),
                Value::Array(kinds) => kinds
                    .iter()
                    .any(|kind| kind.as_str().is_some_and(|s| s.ends_with("ParcelDelivery"))),
                _ => false,
            });
            if is_parcel {
                parcels.push(from_value(value));
            } else {
                for child in map.values() {
                    collect(child, parcels);
                }
            }
        }
        _ => {}
    }
}

fn field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn name(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| field(value, "name"))
        .or_else(|| field(value, "addressLocality"))
}

fn from_value(value: &Value) -> Parcel {
    let order = &value["partOfOrder"];
    let items = &value["itemShipped"];
    let item = if let Some(items) = items.as_array() {
        let names: Vec<_> = items.iter().filter_map(name).collect();
        (!names.is_empty()).then(|| names.join(", "))
    } else {
        name(items)
    };
    let statuses = &value["deliveryStatus"];
    let statuses: Vec<&Value> = statuses
        .as_array()
        .map(|items| items.iter().collect())
        .unwrap_or_else(|| {
            if statuses.is_object() || statuses.is_string() {
                vec![statuses]
            } else {
                Vec::new()
            }
        });
    let steps = statuses
        .into_iter()
        .map(|event| Step {
            name: name(event).map(|name| match name.as_str() {
                "http://schema.org/Delivered" | "https://schema.org/Delivered" => {
                    "Delivered".into()
                }
                "http://schema.org/InTransit" | "https://schema.org/InTransit" => {
                    "In transit".into()
                }
                _ => name,
            }),
            date: field(event, "startDate").or_else(|| field(event, "availableFrom")),
            location: name(&event["location"]),
        })
        .filter(|step| step.name.is_some() || step.date.is_some() || step.location.is_some())
        .collect();
    let action = &value["potentialAction"];
    let tracking_action_url = field(action, "url")
        .or_else(|| field(action, "target"))
        .or_else(|| field(&action["target"], "urlTemplate"));
    Parcel {
        item,
        arrival_from: field(value, "expectedArrivalFrom"),
        arrival_until: field(value, "expectedArrivalUntil")
            .or_else(|| field(value, "expectedArrival")),
        carrier: name(&value["carrier"]).or_else(|| name(&value["provider"])),
        tracking_url: field(value, "trackingUrl").or(tracking_action_url),
        order_url: field(order, "url"),
        order_number: field(order, "orderNumber").or_else(|| field(value, "orderNumber")),
        steps,
        details: value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_single_status_and_explicit_order_id() {
        let parcels = parse(
            r#"<script type="application/ld+json">{"@type":"ParcelDelivery","orderNumber":"123","deliveryStatus":"https://schema.org/Delivered"}</script>"#,
        );
        assert_eq!(parcels[0].order_number.as_deref(), Some("123"));
        assert_eq!(parcels[0].steps[0].name.as_deref(), Some("Delivered"));
    }

    #[test]
    fn uses_explicit_ugreen_order_number_without_using_tracking_as_order_id() {
        let mut message = Message {
            sender: "service.eu@ugreen.com".into(),
            subject: "Your order USB hub has been delivered.".into(),
            body_html: "<p>Order number: #EU-123 Tracking number: XX12345678</p>".into(),
            body_loaded: true,
            ..Default::default()
        };
        assert_eq!(
            parse_message(&message)[0].order_number.as_deref(),
            Some("EU-123")
        );
        message.body_html = "<p>Tracking number: XX12345678</p>".into();
        assert!(parse_message(&message)[0].order_number.is_none());
    }

    #[test]
    fn extracts_parcel_and_timeline_from_json_ld_graph() {
        let html = r#"<script type="application/ld+json">{"@graph":[{"@type":"ParcelDelivery","itemShipped":{"name":"Shoes"},"carrier":{"name":"Parcel Co"},"potentialAction":{"@type":"TrackAction","url":"https://example.org/track"},"deliveryStatus":[{"@type":"DeliveryEvent","name":"Shipped","startDate":"2026-09-29","location":"London"}]}]}</script>"#;
        let parcels = parse(html);
        assert_eq!(parcels.len(), 1);
        assert_eq!(parcels[0].item.as_deref(), Some("Shoes"));
        assert_eq!(
            parcels[0].tracking_url.as_deref(),
            Some("https://example.org/track")
        );
        assert_eq!(parcels[0].steps[0].location.as_deref(), Some("London"));
    }

    #[test]
    fn extracts_microdata_without_mixing_nested_names() {
        let html = r#"<div itemscope itemtype="https://schema.org/ParcelDelivery"><div itemprop="carrier" itemscope itemtype="https://schema.org/Organization"><meta itemprop="name" content="Courier" /></div><div itemprop="itemShipped" itemscope itemtype="https://schema.org/Product"><meta itemprop="name" content="Shoes" /></div><link itemprop="trackingUrl" href="https://example.org/track" /></div>"#;
        let parcels = parse(html);
        assert_eq!(parcels.len(), 1);
        assert_eq!(parcels[0].carrier.as_deref(), Some("Courier"));
        assert_eq!(parcels[0].item.as_deref(), Some("Shoes"));
    }

    #[test]
    fn recognizes_ugreen_shipping_email_without_structured_markup() {
        let message = Message {
            sender: "UGREEN <service.eu@ugreen.com>".into(),
            subject: "Your order USB hub has already been shipped.".into(),
            body_html: "<p>Tracking number: XX12345678 Carrier: WINIT Carrier phone: What's inside</p><a href='https://example.org/track'>Track your order</a>".into(),
            body_loaded: true,
            ..Default::default()
        };
        let parcels = parse_message(&message);
        assert_eq!(parcels.len(), 1);
        assert_eq!(parcels[0].item.as_deref(), Some("USB hub"));
        assert_eq!(parcels[0].carrier.as_deref(), Some("WINIT"));
        assert!(parcels[0].steps.is_empty());
        assert_eq!(parcels[0].details["deliveryStatus"], "In transit");
        assert_eq!(
            parcels[0].tracking_url.as_deref(),
            Some("https://example.org/track")
        );
    }
}
