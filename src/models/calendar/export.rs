use super::{Event, ical::fold};
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}

fn date_property(key: &str, value: &str) -> Option<String> {
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(format!("{key};VALUE=DATE:{}", date.format("%Y%m%d")));
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Some(format!(
            "{key}:{}",
            date.with_timezone(&Utc).format("%Y%m%dT%H%M%SZ")
        ));
    }
    let date = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").ok()?;
    Some(format!("{key}:{}", date.format("%Y%m%dT%H%M%S")))
}

pub fn serialize(event: &Event) -> Option<String> {
    if let Some(calendar) = &event.calendar {
        return Some(super::participation::calendar_with_statuses(
            event, calendar,
        ));
    }
    let start = date_property("DTSTART", &event.start)?;
    let mut lines = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//Brevlada//Calendar//EN".into(),
        "BEGIN:VEVENT".into(),
    ];
    let identity = format!(
        "{}\n{}\n{}",
        event.name,
        event.start,
        event.location.as_deref().unwrap_or_default()
    );
    let uid = event.uid.clone().unwrap_or_else(|| {
        format!(
            "{}@brevlada",
            gtk::glib::compute_checksum_for_string(gtk::glib::ChecksumType::Sha256, &identity)
                .unwrap()
        )
    });
    lines.push(format!("UID:{}", escape(&uid)));
    lines.push(format!("DTSTAMP:{}", Utc::now().format("%Y%m%dT%H%M%SZ")));
    lines.push(start);
    if let Some(end) = &event.end {
        if let Ok(date) = NaiveDate::parse_from_str(end, "%Y-%m-%d") {
            let start = NaiveDate::parse_from_str(&event.start, "%Y-%m-%d").ok()?;
            if date < start {
                return None;
            }
            lines.push(format!(
                "DTEND;VALUE=DATE:{}",
                date.succ_opt()?.format("%Y%m%d")
            ));
        } else {
            if NaiveDate::parse_from_str(&event.start, "%Y-%m-%d").is_ok() {
                return None;
            }
            let ordered = match (
                DateTime::parse_from_rfc3339(&event.start),
                DateTime::parse_from_rfc3339(end),
            ) {
                (Ok(start), Ok(end)) => end > start,
                (Err(_), Err(_)) => {
                    NaiveDateTime::parse_from_str(end, "%Y-%m-%dT%H:%M:%S").ok()?
                        > NaiveDateTime::parse_from_str(&event.start, "%Y-%m-%dT%H:%M:%S").ok()?
                }
                _ => false,
            };
            if !ordered {
                return None;
            }
            lines.push(date_property("DTEND", end)?);
        }
    }
    lines.push(format!("SUMMARY:{}", escape(&event.name)));
    for (key, value) in [
        ("LOCATION", &event.location),
        ("DESCRIPTION", &event.description),
    ] {
        if let Some(value) = value {
            lines.push(format!("{key}:{}", escape(value)));
        }
    }
    if let Some(url) = &event.url {
        lines.push(format!("URL:{url}"));
    }
    if event
        .status
        .as_deref()
        .is_some_and(|status| status.ends_with("EventCancelled"))
    {
        lines.push("STATUS:CANCELLED".into());
    }
    lines.extend(["END:VEVENT".into(), "END:VCALENDAR".into()]);
    let mut result = String::new();
    for line in lines {
        fold(&line, &mut result);
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::calendar::parse_ical as parse;

    #[test]
    fn rejects_reversed_and_mixed_date_ranges() {
        for (start, end) in [
            ("2026-10-03", "2026-10-02"),
            ("2026-10-02T19:00:00", "2026-10-02T18:00:00"),
            ("2026-10-02T19:00:00Z", "2026-10-02T18:00:00Z"),
            ("2026-10-02", "2026-10-02T18:00:00Z"),
            ("2026-10-02T18:00:00", "2026-10-02T19:00:00Z"),
        ] {
            assert!(
                serialize(&Event {
                    name: "Invalid".into(),
                    start: start.into(),
                    end: Some(end.into()),
                    ..Default::default()
                })
                .is_none()
            );
        }
    }

    #[test]
    fn exports_dates_escaping_folding_and_stable_uid() {
        let event = Event {
            name: "å".repeat(100),
            start: "2026-10-02T19:00:00+02:00".into(),
            description: Some("Hello, world;\r\nBEGIN:VALARM".into()),
            ..Default::default()
        };
        let calendar = serialize(&event).unwrap();
        assert!(calendar.contains("DTSTART:20261002T170000Z\r\n"));
        assert!(calendar.contains("DESCRIPTION:Hello\\, world\\;\\nBEGIN:VALARM"));
        assert!(calendar.lines().all(|line| line.len() <= 75));
        let parsed = parse(&calendar);
        assert_eq!(parsed[0].name, event.name);
        assert_eq!(parsed[0].uid, parse(&serialize(&event).unwrap())[0].uid);
        let event = Event {
            name: "Festival".into(),
            start: "2026-10-02".into(),
            end: Some("2026-10-03".into()),
            ..Default::default()
        };
        assert!(
            serialize(&event)
                .unwrap()
                .contains("DTEND;VALUE=DATE:20261004")
        );
        assert!(
            serialize(&Event {
                start: "invalid".into(),
                ..event
            })
            .is_none()
        );
    }
}
