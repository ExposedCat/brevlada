use super::Event;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};

fn timed_range(start: NaiveDateTime, end: Option<NaiveDateTime>, zone: Option<&str>) -> String {
    let mut label = start.format("%a, %e %b %Y · %H:%M").to_string();
    if let Some(end) = end.filter(|end| *end != start) {
        if start.date() == end.date() {
            label.push_str(&format!("–{}", end.format("%H:%M")));
        } else {
            label.push_str(&format!(" – {}", end.format("%a, %e %b %Y · %H:%M")));
        }
    }
    if let Some(zone) = zone {
        label.push_str(&format!(" ({zone})"));
    }
    label
}

pub(super) fn label(event: &Event) -> String {
    if let Ok(start) = NaiveDate::parse_from_str(&event.start, "%Y-%m-%d") {
        let end = event
            .end
            .as_deref()
            .and_then(|end| NaiveDate::parse_from_str(end, "%Y-%m-%d").ok())
            .and_then(|end| {
                if event.calendar.is_some() {
                    end.pred_opt()
                } else {
                    Some(end)
                }
            });
        let mut label = start.format("%a, %e %b %Y").to_string();
        if let Some(end) = end.filter(|end| *end > start) {
            label.push_str(&format!(" – {}", end.format("%a, %e %b %Y")));
        }
        label.push_str(" · All day");
        return label;
    }
    if let Ok(start) = DateTime::parse_from_rfc3339(&event.start) {
        let end = event
            .end
            .as_deref()
            .and_then(|end| DateTime::parse_from_rfc3339(end).ok())
            .map(|end| end.with_timezone(&Local).naive_local());
        return timed_range(start.with_timezone(&Local).naive_local(), end, None);
    }
    if let Ok(start) = NaiveDateTime::parse_from_str(&event.start, "%Y-%m-%dT%H:%M:%S") {
        let end = event
            .end
            .as_deref()
            .and_then(|end| NaiveDateTime::parse_from_str(end, "%Y-%m-%dT%H:%M:%S").ok());
        return timed_range(start, end, event.timezone.as_deref());
    }
    event.start.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn same_day_ranges_show_the_date_once_and_keep_the_timezone() {
        let mut event = Event {
            start: "2026-10-02T17:15:00".into(),
            end: Some("2026-10-02T18:00:00".into()),
            timezone: Some("Europe/Berlin".into()),
            ..Default::default()
        };
        assert_eq!(
            label(&event),
            "Fri,  2 Oct 2026 · 17:15–18:00 (Europe/Berlin)"
        );
        event.start = Local
            .with_ymd_and_hms(2026, 10, 2, 17, 15, 0)
            .unwrap()
            .to_rfc3339();
        event.end = Some(
            Local
                .with_ymd_and_hms(2026, 10, 2, 18, 0, 0)
                .unwrap()
                .to_rfc3339(),
        );
        assert_eq!(label(&event), "Fri,  2 Oct 2026 · 17:15–18:00");
        event.end = Some(
            Local
                .with_ymd_and_hms(2026, 10, 3, 1, 0, 0)
                .unwrap()
                .to_rfc3339(),
        );
        assert_eq!(
            label(&event),
            "Fri,  2 Oct 2026 · 17:15 – Sat,  3 Oct 2026 · 01:00"
        );
    }

    #[test]
    fn calendar_all_day_end_is_exclusive_but_schema_end_is_inclusive() {
        let mut event = Event {
            start: "2026-10-02".into(),
            end: Some("2026-10-03".into()),
            calendar: Some(String::new()),
            ..Default::default()
        };
        assert_eq!(label(&event), "Fri,  2 Oct 2026 · All day");
        event.calendar = None;
        assert_eq!(
            label(&event),
            "Fri,  2 Oct 2026 – Sat,  3 Oct 2026 · All day"
        );
    }
}
