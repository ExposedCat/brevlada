use super::{
    Event,
    rsvp::{actions, web_url},
};
use scraper::{Html, Selector};
use serde_json::Value;

pub fn parse(html: &str) -> Vec<Event> {
    if !html.contains("Event") {
        return Vec::new();
    }
    let document = Html::parse_fragment(html);
    let mut events = Vec::new();
    for script in document.select(&Selector::parse("script[type='application/ld+json']").unwrap()) {
        if let Ok(value) = serde_json::from_str::<Value>(&script.text().collect::<String>()) {
            collect(&value, &mut events);
        }
    }
    for element in document.select(&Selector::parse("[itemscope][itemtype]").unwrap()) {
        if element
            .ancestors()
            .filter_map(scraper::ElementRef::wrap)
            .any(|parent| {
                parent.value().attr("itemtype").is_some_and(|kind| {
                    kind.split_whitespace().any(|kind| {
                        matches!(
                            kind.rsplit('/').next(),
                            Some("EventReservation" | "InviteAction" | "RsvpAction")
                        )
                    })
                })
            })
        {
            continue;
        }
        collect(&crate::models::schema::microdata(&element), &mut events);
    }
    events
}

pub(super) fn kind(value: &Value, name: &str) -> bool {
    match &value["@type"] {
        Value::String(value) => value.rsplit(['/', ':', '#']).next() == Some(name),
        Value::Array(values) => values
            .iter()
            .any(|value| kind(&serde_json::json!({"@type":value}), name)),
        _ => false,
    }
}

pub(super) fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn named(value: &Value) -> Option<String> {
    text(value).or_else(|| text(&value["name"]))
}

fn location(value: &Value) -> Option<String> {
    if let Some(items) = value.as_array() {
        let parts: Vec<_> = items.iter().filter_map(location).collect();
        return (!parts.is_empty()).then(|| parts.join(" · "));
    }
    let mut parts: Vec<String> = named(value).into_iter().collect();
    let address = value.get("address").unwrap_or(value);
    if let Some(address) = text(address) {
        parts.push(address);
    } else {
        for key in [
            "streetAddress",
            "addressLocality",
            "addressRegion",
            "postalCode",
            "addressCountry",
        ] {
            if let Some(part) = named(&address[key]) {
                parts.push(part);
            }
        }
    }
    if parts.is_empty() {
        return text(&value["url"]).filter(|url| web_url(url));
    }
    parts.dedup();
    Some(parts.join(", "))
}

fn collect(value: &Value, events: &mut Vec<Event>) {
    match value {
        Value::Array(items) => items.iter().for_each(|value| collect(value, events)),
        Value::Object(map) => {
            let source = if kind(value, "EventReservation") {
                &value["reservationFor"]
            } else if kind(value, "InviteAction") || kind(value, "RsvpAction") {
                &value["event"]
            } else {
                value
            };
            let is_event = match &source["@type"] {
                Value::String(value) => value
                    .rsplit(['/', ':', '#'])
                    .next()
                    .is_some_and(|kind| kind.ends_with("Event") && kind != "DeliveryEvent"),
                Value::Array(values) => values.iter().any(|value| {
                    value.as_str().is_some_and(|kind| {
                        kind.ends_with("Event") && !kind.ends_with("DeliveryEvent")
                    })
                }),
                _ => false,
            };
            if is_event
                && let (Some(name), Some(start)) =
                    (text(&source["name"]), text(&source["startDate"]))
            {
                let mut event = Event {
                    uid: text(&source["@id"]),
                    name,
                    start,
                    end: text(&source["endDate"]),
                    location: location(&source["location"]),
                    organizer: named(&source["organizer"]),
                    description: text(&source["description"]),
                    url: text(&source["url"]).filter(|url| web_url(url)),
                    status: text(&source["eventStatus"]),
                    ..Default::default()
                };
                actions(&source["potentialAction"], &mut event.rsvp);
                actions(&source["action"], &mut event.rsvp);
                if source != value {
                    actions(&value["potentialAction"], &mut event.rsvp);
                }
                if kind(value, "RsvpAction") {
                    actions(value, &mut event.rsvp);
                }
                if let Some(old) = events.iter_mut().find(|old| old.same_event(&event)) {
                    for action in event.rsvp {
                        if !old.rsvp.contains(&action) {
                            old.rsvp.push(action);
                        }
                    }
                } else {
                    events.push(event);
                }
            } else {
                for child in map.values() {
                    collect(child, events);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_with_rsvp_and_event_subtype() {
        let events = parse(
            r#"<script type="application/ld+json">{"@graph":[{"@type":"EventReservation","reservationFor":{"@type":"MusicEvent","name":"Concert","startDate":"2026-10-02T19:00:00+02:00","location":{"name":"Hall","address":{"streetAddress":"Main Street","addressLocality":"Berlin"}},"potentialAction":[{"@type":"RsvpAction","rsvpResponse":"https://schema.org/RsvpResponseYes","target":{"urlTemplate":"https://example.com/yes"}},{"@type":"RsvpAction","name":"No","target":{"urlTemplate":"https://example.com/no","httpMethod":"POST"}}]}}]}</script>"#,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].location.as_deref(),
            Some("Hall, Main Street, Berlin")
        );
        assert_eq!(events[0].rsvp.len(), 1);
        assert_eq!(events[0].rsvp[0].label, "Yes");
    }

    #[test]
    fn microdata_keeps_nested_scopes_and_deduplicates() {
        let events = parse(
            r#"<div itemscope itemtype="https://schema.org/EventReservation"><div itemprop="reservationFor" itemscope itemtype="https://schema.org/Event"><meta itemprop="name" content="Lunch"><meta itemprop="startDate" content="2026-10-02"><div itemprop="potentialAction" itemscope itemtype="https://schema.org/RsvpAction"><link itemprop="target" href="https://example.com/rsvp"></div></div></div>"#,
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].rsvp[0].url, "https://example.com/rsvp");
        assert!(parse("<script type='application/ld+json'>{broken}</script>").is_empty());
    }
}
