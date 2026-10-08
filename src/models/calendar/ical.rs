use super::{Attendee, Event};
use chrono::{DateTime, NaiveDate, NaiveDateTime};

pub(super) fn unescape(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n' | 'N') => result.push('\n'),
                Some(ch) => result.push(ch),
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

pub(super) fn fold(line: &str, result: &mut String) {
    let mut bytes = 0;
    for ch in line.chars() {
        if bytes + ch.len_utf8() > 75 {
            result.push_str("\r\n ");
            bytes = 1;
        }
        result.push(ch);
        bytes += ch.len_utf8();
    }
    result.push_str("\r\n");
}

pub(super) fn property(line: &str) -> Option<(&str, &str, &str)> {
    let mut quoted = false;
    let split = line.char_indices().find_map(|(index, ch)| {
        if ch == '"' {
            quoted = !quoted;
        }
        (ch == ':' && !quoted).then_some(index)
    })?;
    let (head, value) = line.split_at(split);
    let key = head.split(';').next()?;
    Some((key, head, &value[1..]))
}

pub(super) fn parameter(head: &str, key: &str) -> Option<String> {
    head.split(';').skip(1).find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.eq_ignore_ascii_case(key)
            .then(|| value.trim_matches('"').to_owned())
    })
}

pub(super) fn date(value: &str) -> Option<String> {
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y%m%d") {
        return Some(date.format("%Y-%m-%d").to_string());
    }
    if let Some(value) = value.strip_suffix('Z') {
        let date = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
        return Some(format!("{}Z", date.format("%Y-%m-%dT%H:%M:%S")));
    }
    let date = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    Some(date.format("%Y-%m-%dT%H:%M:%S").to_string())
}

pub(super) fn calendar_email(value: &str) -> Option<String> {
    let value = value
        .get(..7)
        .filter(|prefix| prefix.eq_ignore_ascii_case("mailto:"))
        .and_then(|_| value.get(7..))?;
    let parsed = mailparse::addrparse(value).ok()?;
    if parsed.len() != 1 {
        return None;
    }
    match &parsed[0] {
        mailparse::MailAddr::Single(address)
            if address.addr == value
                && !value
                    .chars()
                    .any(|ch| ch.is_control() || ch.is_whitespace() || "<>:,;?".contains(ch)) =>
        {
            Some(value.to_owned())
        }
        _ => None,
    }
}

pub(super) fn unfold(source: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in source.lines() {
        if let Some(rest) = line.strip_prefix([' ', '\t'])
            && let Some(previous) = lines.last_mut()
        {
            previous.push_str(rest);
        } else {
            lines.push(line.to_owned());
        }
    }
    lines
}

pub fn parse(source: &str) -> Vec<Event> {
    let lines = unfold(source);
    if !lines
        .first()
        .is_some_and(|line| line.eq_ignore_ascii_case("BEGIN:VCALENDAR"))
        || !lines
            .last()
            .is_some_and(|line| line.eq_ignore_ascii_case("END:VCALENDAR"))
    {
        return Vec::new();
    }
    let method = lines.iter().find_map(|line| {
        let (key, _, value) = property(line)?;
        key.eq_ignore_ascii_case("METHOD")
            .then(|| value.to_ascii_uppercase())
    });
    let invitation = lines
        .iter()
        .any(|line| line.eq_ignore_ascii_case("METHOD:REQUEST"));
    let mut common = Vec::new();
    let mut blocks = Vec::new();
    let mut block = Vec::new();
    let mut inside = false;
    for line in lines.iter().skip(1).take(lines.len().saturating_sub(2)) {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            inside = true;
        }
        if inside {
            block.push(line.clone());
        } else {
            common.push(line.clone());
        }
        if inside && line.eq_ignore_ascii_case("END:VEVENT") {
            inside = false;
            blocks.push(std::mem::take(&mut block));
        }
    }
    let mut events = Vec::new();
    for block in &blocks {
        let mut event = Event {
            invitation,
            method: method.clone(),
            ..Default::default()
        };
        let mut depth = 0;
        for line in block.iter().skip(1).take(block.len().saturating_sub(2)) {
            let Some((key, head, value)) = property(line) else {
                continue;
            };
            if key.eq_ignore_ascii_case("BEGIN") {
                depth += 1;
                continue;
            }
            if key.eq_ignore_ascii_case("END") {
                depth -= 1;
                continue;
            }
            if depth != 0 {
                continue;
            }
            match key.to_ascii_uppercase().as_str() {
                "UID" => event.uid = Some(unescape(value)),
                "RECURRENCE-ID" => event.recurrence_id = Some(line.clone()),
                "SEQUENCE" => event.sequence = value.parse().unwrap_or_default(),
                "DTSTAMP" => {
                    event.stamp = date(value)
                        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                        .map(|value| value.timestamp())
                        .unwrap_or_default()
                }
                "ATTENDEE" => {
                    if let Some(email) = calendar_email(value) {
                        event.attendees.push(Attendee {
                            email,
                            name: parameter(head, "CN"),
                            status: parameter(head, "PARTSTAT")
                                .unwrap_or_else(|| "NEEDS-ACTION".into())
                                .to_ascii_uppercase(),
                        });
                    }
                }
                "SUMMARY" => event.name = unescape(value),
                "DTSTART" => {
                    event.start = date(value).unwrap_or_default();
                    event.timezone = parameter(head, "TZID");
                }
                "DTEND" => event.end = date(value),
                "LOCATION" => event.location = Some(unescape(value)),
                "ORGANIZER" => {
                    event.organizer_email = calendar_email(value);
                    event.organizer = Some(
                        parameter(head, "CN")
                            .unwrap_or_else(|| value.trim_start_matches("mailto:").to_owned()),
                    )
                }
                "DESCRIPTION" => event.description = Some(unescape(value)),
                "URL" if value.starts_with("https://") || value.starts_with("http://") => {
                    event.url = Some(value.to_owned())
                }
                "STATUS" => event.status = Some(value.to_owned()),
                "REQUEST-STATUS" if !value.starts_with("2.") => {
                    event.status = Some("REPLY-FAILED".into())
                }
                _ => {}
            }
        }
        if event.start.is_empty()
            && !(matches!(event.method.as_deref(), Some("REPLY" | "CANCEL")) && event.uid.is_some())
        {
            continue;
        }
        if event.status.as_deref() == Some("REPLY-FAILED") {
            event.attendees.clear();
        }
        if event.name.is_empty() {
            event.name = "Calendar event".into();
        }
        let mut calendar = String::new();
        fold("BEGIN:VCALENDAR", &mut calendar);
        for line in &common {
            fold(line, &mut calendar);
        }
        for related in &blocks {
            let related_uid = related.iter().find_map(|line| {
                let (key, _, value) = property(line)?;
                key.eq_ignore_ascii_case("UID").then(|| unescape(value))
            });
            if related_uid == event.uid && (event.uid.is_some() || related == block) {
                for line in related {
                    fold(line, &mut calendar);
                }
            }
        }
        fold("END:VCALENDAR", &mut calendar);
        event.calendar = Some(calendar);
        events.push(event);
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurring_request_preserves_each_instance_and_saves_the_series() {
        let source = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:series\r\nDTSTART:20261002T120000Z\r\nRRULE:FREQ=DAILY;COUNT=2\r\nSUMMARY:Meeting\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:series\r\nRECURRENCE-ID:20261003T120000Z\r\nDTSTART:20261003T130000Z\r\nSUMMARY:Moved meeting\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse(source);
        assert_eq!(events.len(), 2);
        assert!(!events[0].same_event(&events[1]));
        let message = crate::models::Message {
            calendar_events: events.clone(),
            ..Default::default()
        };
        assert_eq!(super::super::events(&message).len(), 2);
        for event in events {
            assert_eq!(parse(&event.to_ical().unwrap()).len(), 2);
        }
    }

    #[test]
    fn preserves_timezone_recurrence_attendees_and_ignores_alarms() {
        let source = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:one\r\nDTSTART;TZID=Europe/Berlin:20261002T190000\r\nSUMMARY:Long\r\n  title\r\nRRULE:FREQ=WEEKLY\r\nORGANIZER;CN=\"Team: Calendar\":mailto:team@example.com\r\nATTENDEE;RSVP=TRUE:mailto:me@example.com\r\nBEGIN:VALARM\r\nDESCRIPTION:Alarm\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:two\r\nDTSTART;VALUE=DATE:20261003\r\nSUMMARY:Second\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let events = parse(source);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].name, "Long title");
        assert_eq!(events[0].organizer.as_deref(), Some("Team: Calendar"));
        assert_eq!(events[0].timezone.as_deref(), Some("Europe/Berlin"));
        assert!(events[0].description.is_none());
        assert!(events[0].invitation);
        let saved = events[0].to_ical().unwrap();
        assert!(saved.contains("RRULE:FREQ=WEEKLY"));
        assert!(saved.contains("BEGIN:VTIMEZONE"));
        assert!(saved.contains("ATTENDEE;RSVP=TRUE"));
        assert!(!saved.contains("UID:two"));
    }
}
