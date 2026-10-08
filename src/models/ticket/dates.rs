use chrono::{DateTime, NaiveDate, NaiveDateTime};

pub(super) fn normalize(value: String) -> String {
    DateTime::parse_from_rfc3339(&value)
        .or_else(|_| DateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S%z"))
        .map(|date| date.to_rfc3339())
        .unwrap_or(value)
}

pub fn date_label(value: &str) -> String {
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return date.format("%a, %-d %b %Y · %H:%M (UTC%:z)").to_string();
    }
    for format in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(value, format) {
            return date.format("%a, %-d %b %Y · %H:%M").to_string();
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return date.format("%a, %-d %b %Y").to_string();
    }
    value.to_owned()
}

pub fn day_label(value: &str) -> String {
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return date.format("%-d %b %Y").to_string();
    }
    for format in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(value, format) {
            return date.format("%-d %b %Y").to_string();
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return date.format("%-d %b %Y").to_string();
    }
    value.to_owned()
}

pub fn time_label(value: &str) -> Option<String> {
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Some(date.format("%H:%M").to_string());
    }
    for format in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(value, format) {
            return Some(date.format("%H:%M").to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journey_times_keep_the_offset_at_each_stop() {
        assert_eq!(
            date_label("2026-10-08T13:50:00+02:00"),
            "Thu, 8 Oct 2026 · 13:50 (UTC+02:00)"
        );
        assert_eq!(
            date_label("2026-10-08T15:00:00+01:00"),
            "Thu, 8 Oct 2026 · 15:00 (UTC+01:00)"
        );
    }

    #[test]
    fn invoice_offsets_match_confirmation_offsets() {
        assert_eq!(
            normalize("2025-11-27T13:00:00+0100".into()),
            "2025-11-27T13:00:00+01:00"
        );
    }

    #[test]
    fn compact_timeline_dates_keep_stop_time() {
        assert_eq!(
            time_label("2026-10-08T13:50:00+02:00").as_deref(),
            Some("13:50")
        );
        assert_eq!(
            time_label("2026-10-08T15:00:00+01:00").as_deref(),
            Some("15:00")
        );
        assert_eq!(day_label("2026-10-08T13:50:00+02:00"), "8 Oct 2026");
        assert_eq!(day_label("2026-10-08"), "8 Oct 2026");
        assert_eq!(time_label("2026-10-08"), None);
    }
}
