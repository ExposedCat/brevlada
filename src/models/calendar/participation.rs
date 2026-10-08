use super::{
    Event,
    ical::{calendar_email, fold, property, unescape, unfold},
};

pub(super) fn calendar_with_statuses(event: &Event, calendar: &str) -> String {
    let lines = unfold(calendar);
    let mut result = String::new();
    let mut index = 0;
    while index < lines.len() {
        if !lines[index].eq_ignore_ascii_case("BEGIN:VEVENT") {
            fold(&lines[index], &mut result);
            index += 1;
            continue;
        }
        let end = lines[index..]
            .iter()
            .position(|line| line.eq_ignore_ascii_case("END:VEVENT"))
            .map(|offset| index + offset)
            .unwrap_or(lines.len() - 1);
        let block = &lines[index..=end];
        let uid = block.iter().find_map(|line| {
            let (key, _, value) = property(line)?;
            key.eq_ignore_ascii_case("UID").then(|| unescape(value))
        });
        let recurrence = block.iter().find(|line| {
            property(line).is_some_and(|(key, _, _)| key.eq_ignore_ascii_case("RECURRENCE-ID"))
        });
        let selected =
            uid == event.uid && recurrence.map(String::as_str) == event.recurrence_id.as_deref();
        for line in block {
            if selected
                && let Some((key, head, value)) = property(line)
                && key.eq_ignore_ascii_case("ATTENDEE")
                && let Some(email) = calendar_email(value)
                && let Some(attendee) = event
                    .attendees
                    .iter()
                    .find(|attendee| attendee.email.eq_ignore_ascii_case(&email))
                && matches!(
                    attendee.status.as_str(),
                    "ACCEPTED" | "DECLINED" | "TENTATIVE" | "NEEDS-ACTION" | "DELEGATED"
                )
            {
                static PARTSTAT: std::sync::LazyLock<regex::Regex> =
                    std::sync::LazyLock::new(|| regex::Regex::new("(?i);PARTSTAT=[^;]*").unwrap());
                let head = PARTSTAT.replace(head, "");
                fold(
                    &format!("{head};PARTSTAT={}:{value}", attendee.status),
                    &mut result,
                );
            } else {
                fold(line, &mut result);
            }
        }
        index = end + 1;
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn exports_updated_attendance_without_changing_other_instances() {
        let source = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:series\r\nDTSTART:20261002T120000Z\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:me@example.com\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:series\r\nRECURRENCE-ID:20261003T120000Z\r\nDTSTART:20261003T130000Z\r\nATTENDEE;CN=Me;PARTSTAT=NEEDS-ACTION:mailto:me@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let mut event = super::super::parse_ical(source).remove(1);
        event.attendees[0].status = "ACCEPTED".into();
        let saved = event.to_ical().unwrap();
        let parsed = super::super::parse_ical(&saved);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].attendees[0].status, "NEEDS-ACTION");
        assert_eq!(parsed[1].attendees[0].status, "ACCEPTED");
        assert_eq!(parsed[1].attendees[0].name.as_deref(), Some("Me"));
    }
}
