use serde_json::Value;

pub(super) fn kind(value: &Value, name: &str) -> bool {
    match &value["@type"] {
        Value::String(value) => term(value) == name,
        Value::Array(values) => values
            .iter()
            .any(|value| value.as_str().is_some_and(|value| term(value) == name)),
        _ => false,
    }
}

pub(super) fn term(value: &str) -> &str {
    value.rsplit(['/', ':', '#']).next().unwrap_or(value)
}

pub(super) fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => (!text.trim().is_empty()).then(|| text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub(super) fn named(value: &Value) -> Option<String> {
    text(value).or_else(|| text(&value["name"]))
}

pub fn web_url(value: &str) -> bool {
    gtk::glib::Uri::parse(value, gtk::glib::UriFlags::NONE).is_ok_and(|uri| {
        matches!(uri.scheme().as_str(), "http" | "https")
            && uri.host().is_some_and(|host| !host.is_empty())
    })
}

pub(super) fn url(value: &Value) -> Option<String> {
    text(value).filter(|value| web_url(value))
}

pub(super) fn place(value: &Value) -> Option<String> {
    let mut parts: Vec<_> = named(value).into_iter().collect();
    if let Some(code) = text(&value["iataCode"])
        && !parts.iter().any(|part| part == &code)
    {
        if let Some(name) = parts.first_mut() {
            name.push_str(&format!(" ({code})"));
        } else {
            parts.push(code);
        }
    }
    let address = &value["address"];
    if let Some(address) = text(address) {
        parts.push(address);
    } else {
        for key in ["streetAddress", "addressLocality", "addressCountry"] {
            if let Some(part) = named(&address[key])
                && !parts.contains(&part)
            {
                parts.push(part);
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}

pub(super) fn seat(value: &Value) -> Option<String> {
    let mut parts = Vec::new();
    for (key, label) in [
        ("seatSection", "Section"),
        ("seatRow", "Row"),
        ("seatNumber", "Seat"),
    ] {
        if let Some(part) = text(&value[key]) {
            parts.push(format!("{label} {part}"));
        }
    }
    if let Some(part) = text(&value["seatingType"]) {
        parts.push(part);
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}
