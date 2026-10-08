use super::{
    Event, Response,
    ical::{fold, property},
};
use anyhow::{Context, Result, ensure};

pub fn reply(event: &Event, email: &str, response: Response) -> Result<String> {
    ensure!(
        event.can_reply(email),
        "This account is not an attendee of this invitation"
    );
    let source = event
        .calendar
        .as_deref()
        .context("Invitation data is unavailable")?;
    let lines = super::ical::unfold(source);
    let mut result = String::new();
    for line in [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//Brevlada//Calendar//EN",
        "METHOD:REPLY",
    ] {
        fold(line, &mut result);
    }
    let mut timezone = false;
    for line in &lines {
        if line.eq_ignore_ascii_case("BEGIN:VTIMEZONE") {
            timezone = true;
        }
        if timezone {
            fold(line, &mut result);
        }
        if line.eq_ignore_ascii_case("END:VTIMEZONE") {
            timezone = false;
        }
    }
    let mut block = Vec::new();
    let mut inside = false;
    let mut selected = None;
    for line in &lines {
        if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
            inside = true;
        }
        if inside {
            block.push(line.as_str());
        }
        if line.eq_ignore_ascii_case("END:VEVENT") && inside {
            let uid = block.iter().find_map(|line| {
                let (key, _, value) = property(line)?;
                key.eq_ignore_ascii_case("UID")
                    .then(|| super::ical::unescape(value))
            });
            let recurrence = block
                .iter()
                .find(|line| {
                    property(line)
                        .is_some_and(|(key, _, _)| key.eq_ignore_ascii_case("RECURRENCE-ID"))
                })
                .copied();
            if uid == event.uid && recurrence == event.recurrence_id.as_deref() {
                selected = Some(block.clone());
                break;
            }
            block.clear();
            inside = false;
        }
    }
    let selected = selected.context("The requested event instance is unavailable")?;
    fold("BEGIN:VEVENT", &mut result);
    let mut depth = 0;
    for line in selected
        .iter()
        .skip(1)
        .take(selected.len().saturating_sub(2))
    {
        let Some((key, _, _)) = property(line) else {
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
        if depth == 0
            && matches!(
                key.to_ascii_uppercase().as_str(),
                "UID"
                    | "ORGANIZER"
                    | "SEQUENCE"
                    | "DTSTART"
                    | "DTEND"
                    | "DURATION"
                    | "RECURRENCE-ID"
                    | "SUMMARY"
                    | "LOCATION"
                    | "RRULE"
                    | "RDATE"
                    | "EXDATE"
            )
        {
            fold(line, &mut result);
        }
    }
    fold(
        &format!("DTSTAMP:{}", chrono::Utc::now().format("%Y%m%dT%H%M%SZ")),
        &mut result,
    );
    fold(
        &format!("ATTENDEE;PARTSTAT={}:mailto:{email}", response.status()),
        &mut result,
    );
    fold("REQUEST-STATUS:2.0;Success", &mut result);
    fold("END:VEVENT", &mut result);
    fold("END:VCALENDAR", &mut result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_keeps_identity_and_sends_only_the_current_attendee() {
        let raw = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:meeting\r\nSEQUENCE:4\r\nRECURRENCE-ID:20261002T120000Z\r\nDTSTART:20261002T130000Z\r\nORGANIZER:mailto:host@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:me@example.com\r\nATTENDEE:mailto:other@example.com\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Reminder\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let event = super::super::parse_ical(raw).remove(0);
        for response in [Response::Accepted, Response::Tentative, Response::Declined] {
            let source = reply(&event, "me@example.com", response).unwrap();
            let parsed = super::super::parse_ical(&source).remove(0);
            assert_eq!(parsed.uid, event.uid);
            assert_eq!(parsed.recurrence_id, event.recurrence_id);
            assert_eq!(parsed.sequence, 4);
            assert_eq!(parsed.method.as_deref(), Some("REPLY"));
            assert_eq!(parsed.attendees.len(), 1);
            assert_eq!(parsed.attendees[0].status, response.status());
            assert!(!source.contains("VALARM"));
        }
        assert!(reply(&event, "stranger@example.com", Response::Accepted).is_err());
    }
}
