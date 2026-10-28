use super::{Event, date_property, escape, fold};
use crate::models::calendar::ical::{parse, property, unfold};

fn date(key: &str, value: &str, timezone: Option<&str>) -> Option<String> {
    let line = date_property(key, value)?;
    if let Some(timezone) = timezone
        && chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").is_ok()
    {
        return Some(line.replacen(':', &format!(";TZID={timezone}:"), 1));
    }
    Some(line)
}

pub(super) fn enrich(event: &Event, calendar: &str) -> Option<String> {
    let original = parse(calendar)
        .into_iter()
        .find(|original| original.same_event(event))?;
    let mut changes = Vec::new();
    if event.start != original.start || event.timezone != original.timezone {
        changes.push((
            "DTSTART",
            date("DTSTART", &event.start, event.timezone.as_deref())?,
        ));
    }
    if (event.end != original.end || event.timezone != original.timezone)
        && let Some(end) = &event.end
    {
        changes.push(("DTEND", date("DTEND", end, event.timezone.as_deref())?));
    }
    for (key, value, old) in [
        ("LOCATION", &event.location, &original.location),
        ("DESCRIPTION", &event.description, &original.description),
        ("URL", &event.url, &original.url),
    ] {
        if value != old
            && let Some(value) = value
        {
            let value = if key == "URL" {
                value.clone()
            } else {
                escape(value)
            };
            changes.push((key, format!("{key}:{value}")));
        }
    }
    if (event.organizer != original.organizer || event.organizer_email != original.organizer_email)
        && let Some(email) = &event.organizer_email
    {
        let name = event.organizer.as_deref().unwrap_or(email);
        changes.push((
            "ORGANIZER",
            format!(
                "ORGANIZER;CN=\"{}\":mailto:{email}",
                escape(name).replace('"', "'")
            ),
        ));
    }
    if changes.is_empty() {
        return Some(calendar.to_owned());
    }
    let mut result = String::new();
    let mut selected = false;
    let mut depth = 0;
    let mut remaining = Vec::new();
    let lines = unfold(calendar);
    for (index, line) in lines.iter().enumerate() {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            let block = &lines[index..];
            let uid = block
                .iter()
                .take_while(|line| !line.eq_ignore_ascii_case("END:VEVENT"))
                .find_map(|line| {
                    let (key, _, value) = property(line)?;
                    key.eq_ignore_ascii_case("UID")
                        .then(|| super::super::ical::unescape(value))
                });
            let recurrence = block
                .iter()
                .take_while(|line| !line.eq_ignore_ascii_case("END:VEVENT"))
                .find(|line| {
                    property(line)
                        .is_some_and(|(key, _, _)| key.eq_ignore_ascii_case("RECURRENCE-ID"))
                });
            selected = uid == event.uid
                && recurrence.map(String::as_str) == event.recurrence_id.as_deref();
            depth = 0;
            remaining = changes.iter().collect();
        } else if selected && line.eq_ignore_ascii_case("END:VEVENT") {
            for (_, replacement) in remaining.drain(..) {
                fold(replacement, &mut result);
            }
            selected = false;
        } else if selected && let Some((key, _, _)) = property(line) {
            if key.eq_ignore_ascii_case("BEGIN") {
                depth += 1;
            } else if key.eq_ignore_ascii_case("END") {
                depth -= 1;
            } else if depth == 0
                && let Some(position) = remaining
                    .iter()
                    .position(|(name, _)| key.eq_ignore_ascii_case(name))
            {
                let (_, replacement) = remaining.remove(position);
                fold(replacement, &mut result);
                continue;
            }
        }
        fold(line, &mut result);
    }
    Some(result)
}
