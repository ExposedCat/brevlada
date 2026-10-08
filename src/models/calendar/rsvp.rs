use super::Rsvp;
use super::schema::{kind, text};
use serde_json::Value;

pub(super) fn web_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.contains(['\r', '\n', '{', '}'])
}

pub(super) fn actions(value: &Value, rsvp: &mut Vec<Rsvp>) {
    if let Some(items) = value.as_array() {
        for item in items {
            actions(item, rsvp);
        }
        return;
    }
    if !kind(value, "RsvpAction") {
        return;
    }
    let target = value
        .get("target")
        .or_else(|| value.get("handler"))
        .unwrap_or(&Value::Null);
    let targets: Vec<_> = target
        .as_array()
        .map(|items| items.iter().collect())
        .unwrap_or_else(|| vec![target]);
    let url = targets
        .into_iter()
        .find_map(|target| {
            let method = text(&target["httpMethod"]).or_else(|| text(&target["method"]));
            if method.is_some_and(|method| {
                !method
                    .rsplit('/')
                    .next()
                    .unwrap_or(&method)
                    .eq_ignore_ascii_case("GET")
            }) {
                return None;
            }
            text(target)
                .or_else(|| text(&target["urlTemplate"]))
                .or_else(|| text(&target["url"]))
                .filter(|url| web_url(url))
        })
        .or_else(|| {
            target
                .is_null()
                .then(|| text(&value["url"]).filter(|url| web_url(url)))
                .flatten()
        });
    let Some(url) = url else {
        return;
    };
    let label = match text(&value["rsvpResponse"])
        .or_else(|| text(&value["attendance"]))
        .as_deref()
        .and_then(|s| s.rsplit('/').next())
    {
        Some("RsvpResponseYes" | "Yes") => "Yes".into(),
        Some("RsvpResponseNo" | "No") => "No".into(),
        Some("RsvpResponseMaybe" | "Maybe") => "Maybe".into(),
        _ => text(&value["name"]).unwrap_or_else(|| "RSVP".into()),
    };
    let action = Rsvp { label, url };
    if !rsvp.contains(&action) {
        rsvp.push(action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_handlers_use_get_and_reject_unsupported_targets() {
        let mut result = Vec::new();
        for (method, url) in [
            ("GET", "https://example.com/yes"),
            ("POST", "https://example.com/no"),
            ("GET", "file:///tmp/event"),
            ("GET", "https://example.com/{response}"),
        ] {
            actions(
                &json!({"@type":"RsvpAction", "attendance":"https://schema.org/RsvpAttendance/Yes", "handler":{"method":method, "url":url}}),
                &mut result,
            );
        }
        assert_eq!(
            result,
            [Rsvp {
                label: "Yes".into(),
                url: "https://example.com/yes".into()
            }]
        );
    }
}
