use super::Event;
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone};

pub(super) fn same_start(left: &Event, right: &Event) -> bool {
    match (
        DateTime::parse_from_rfc3339(&left.start),
        DateTime::parse_from_rfc3339(&right.start),
    ) {
        (Ok(left), Ok(right)) => left == right,
        (Ok(aware), Err(_)) if right.timezone.is_none() => {
            naive(&right.start) == Some(aware.naive_local())
        }
        (Err(_), Ok(aware)) if left.timezone.is_none() => {
            naive(&left.start) == Some(aware.naive_local())
        }
        (Err(_), Err(_)) => {
            left.start == right.start
                && match (&left.timezone, &right.timezone) {
                    (Some(left), Some(right)) => left == right,
                    _ => true,
                }
        }
        _ => false,
    }
}

fn naive(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").ok()
}

pub(super) fn insert(events: &mut Vec<Event>, event: Event) {
    if let Some(old) = events.iter_mut().find(|old| old.same_event(&event)) {
        if old.calendar.is_none() && event.calendar.is_some() {
            let previous = std::mem::replace(old, event);
            enrich(old, &previous);
        } else {
            enrich(old, &event);
        }
    } else {
        events.push(event);
    }
}

fn richer(target: &mut Option<String>, source: &Option<String>) {
    if let Some(value) = source
        && !value.trim().is_empty()
        && target
            .as_ref()
            .is_none_or(|old| value.trim().chars().count() > old.trim().chars().count())
    {
        *target = Some(value.clone());
    }
}

pub(super) fn enrich(target: &mut Event, source: &Event) {
    target.uid = target.uid.take().or_else(|| source.uid.clone());
    target.method = target.method.take().or_else(|| source.method.clone());
    target.organizer_email = target
        .organizer_email
        .take()
        .or_else(|| source.organizer_email.clone());
    target.status = target.status.take().or_else(|| source.status.clone());
    target.invitation |= source.invitation;
    for (target, source) in [
        (&mut target.location, &source.location),
        (&mut target.organizer, &source.organizer),
        (&mut target.description, &source.description),
        (&mut target.url, &source.url),
    ] {
        richer(target, source);
    }
    if target.end.is_none() {
        target.end = source.end.clone();
        if target.calendar.is_some()
            && source.calendar.is_none()
            && let Some(date) = target
                .end
                .as_deref()
                .and_then(|end| NaiveDate::parse_from_str(end, "%Y-%m-%d").ok())
        {
            target.end = date.succ_opt().map(|date| date.to_string());
        }
    }
    if target.timezone.is_none() && naive(&target.start).is_some() {
        if DateTime::parse_from_rfc3339(&source.start).is_ok() {
            target.start = source.start.clone();
            if let Some(end) = &source.end {
                target.end = Some(end.clone());
            }
        } else if source.timezone.is_some() {
            target.timezone = source.timezone.clone();
        }
    }
    if target.timezone.is_none()
        && let Ok(start) = DateTime::parse_from_rfc3339(&target.start)
        && let Some(end) = target.end.as_deref().and_then(naive)
    {
        target.end = Some(
            start
                .offset()
                .from_local_datetime(&end)
                .single()
                .unwrap()
                .to_rfc3339(),
        );
    }
    for attendee in &source.attendees {
        if let Some(old) = target
            .attendees
            .iter_mut()
            .find(|old| old.email.eq_ignore_ascii_case(&attendee.email))
        {
            richer(&mut old.name, &attendee.name);
        } else {
            target.attendees.push(attendee.clone());
        }
    }
    for action in &source.rsvp {
        if !target.rsvp.contains(action) {
            target.rsvp.push(action.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        Message,
        calendar::{events, parse_ical},
    };

    fn appointment() -> Event {
        parse_ical("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:PUBLISH\r\nBEGIN:VEVENT\r\nUID:booking@example.com\r\nDTSTART:20270125T093000\r\nDTEND:20270125T100000\r\nSUMMARY:Appointment\r\nLOCATION:Clinic\r\nORGANIZER;CN=Doctor:mailto:clinic@example.com\r\nDESCRIPTION:Booking confirmed\r\nX-BOOKING-ID:123\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Reminder\r\nTRIGGER:-PT30M\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").remove(0)
    }

    #[test]
    fn merges_attachment_and_schema_into_one_rich_export_and_survives_cache_reload() {
        let mut message = Message {
            calendar_events: vec![appointment()],
            body_loaded: true,
            body_html: r#"<script type="application/ld+json">{"@type":"EventReservation","reservationFor":{"@type":"Event","name":"Appointment","startDate":"2027-01-25T09:30:00+01:00","endDate":"2027-01-25T10:00:00+01:00","location":"Clinic, Main Street 1, Prague, CZ","url":"https://example.com/booking","potentialAction":{"@type":"RsvpAction","name":"Confirm","target":"https://example.com/confirm"}}}</script>"#.into(),
            ..Default::default()
        };
        let merged = events(&message);
        assert_eq!(merged.len(), 1);
        let event = &merged[0];
        assert_eq!(event.uid, appointment().uid);
        assert_eq!(event.organizer_email, appointment().organizer_email);
        assert_eq!(event.description, appointment().description);
        assert_eq!(event.start, "2027-01-25T09:30:00+01:00");
        assert_eq!(event.end.as_deref(), Some("2027-01-25T10:00:00+01:00"));
        assert_eq!(
            event.location.as_deref(),
            Some("Clinic, Main Street 1, Prague, CZ")
        );
        assert_eq!(event.rsvp.len(), 1);
        let saved = event.to_ical().unwrap();
        let parsed = parse_ical(&saved);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].uid, event.uid);
        assert_eq!(parsed[0].location, event.location);
        assert_eq!(parsed[0].description, event.description);
        assert_eq!(parsed[0].url, event.url);
        assert_eq!(parsed[0].organizer_email, event.organizer_email);
        assert_eq!(parsed[0].start, "2027-01-25T08:30:00Z");
        assert_eq!(parsed[0].end.as_deref(), Some("2027-01-25T09:00:00Z"));
        assert!(saved.contains("X-BOOKING-ID:123\r\n"));
        assert!(saved.contains("DESCRIPTION:Reminder\r\n"));
        assert!(saved.contains("TRIGGER:-PT30M\r\n"));
        message.calendar_events =
            serde_json::from_str(&serde_json::to_string(&merged).unwrap()).unwrap();
        assert_eq!(events(&message), merged);
        message.prepare_display();
        assert_eq!(events(&message), merged);
    }

    #[test]
    fn merges_floating_duplicates_in_either_order_and_keeps_attendees_and_actions() {
        let mut rich = appointment();
        rich.attendees.push(super::super::Attendee {
            email: "guest@example.com".into(),
            name: Some("Guest".into()),
            status: "ACCEPTED".into(),
        });
        rich.rsvp.push(super::super::Rsvp {
            label: "Confirm".into(),
            url: "https://example.com/confirm".into(),
        });
        let sparse = Event {
            name: rich.name.clone(),
            start: rich.start.clone(),
            description: Some("Booking confirmed with additional instructions".into()),
            ..Default::default()
        };
        for input in [
            vec![rich.clone(), sparse.clone()],
            vec![sparse.clone(), rich.clone()],
        ] {
            let mut merged = Vec::new();
            for event in input {
                insert(&mut merged, event);
            }
            assert_eq!(merged.len(), 1);
            assert_eq!(merged[0].uid, rich.uid);
            assert_eq!(merged[0].calendar, rich.calendar);
            assert_eq!(merged[0].attendees, rich.attendees);
            assert_eq!(merged[0].rsvp, rich.rsvp);
            assert_eq!(merged[0].description, sparse.description);
            assert_eq!(
                parse_ical(&merged[0].to_ical().unwrap())[0].description,
                sparse.description
            );
        }
    }

    #[test]
    fn keeps_distinct_ids_instances_times_and_known_timezones_separate() {
        let floating = Event {
            name: "Appointment".into(),
            start: "2027-01-25T09:30:00".into(),
            ..Default::default()
        };
        let aware = Event {
            start: "2027-01-25T09:30:00+01:00".into(),
            ..floating.clone()
        };
        assert!(floating.same_event(&aware));
        assert!(aware.same_event(&floating));
        assert!(aware.same_event(&Event {
            start: "2027-01-25T08:30:00Z".into(),
            ..floating.clone()
        }));
        for (left, right) in [
            (
                floating.clone(),
                Event {
                    name: "Other appointment".into(),
                    ..floating.clone()
                },
            ),
            (
                floating.clone(),
                Event {
                    start: "2027-01-25T10:30:00+01:00".into(),
                    ..floating.clone()
                },
            ),
            (
                aware.clone(),
                Event {
                    start: "2027-01-25T09:30:00+02:00".into(),
                    ..floating.clone()
                },
            ),
            (
                Event {
                    uid: Some("one".into()),
                    ..floating.clone()
                },
                Event {
                    uid: Some("two".into()),
                    ..floating.clone()
                },
            ),
            (
                floating.clone(),
                Event {
                    recurrence_id: Some("RECURRENCE-ID:20270125T093000".into()),
                    ..floating.clone()
                },
            ),
            (
                Event {
                    timezone: Some("Europe/Prague".into()),
                    ..floating.clone()
                },
                Event {
                    timezone: Some("America/New_York".into()),
                    ..floating.clone()
                },
            ),
            (
                Event {
                    timezone: Some("America/New_York".into()),
                    ..floating.clone()
                },
                aware,
            ),
        ] {
            assert!(!left.same_event(&right));
            assert!(!right.same_event(&left));
        }
    }

    #[test]
    fn fills_missing_end_using_the_explicit_start_offset() {
        let aware = Event {
            name: "Appointment".into(),
            start: "2027-01-25T09:30:00+01:00".into(),
            ..Default::default()
        };
        let floating = Event {
            start: "2027-01-25T09:30:00".into(),
            end: Some("2027-01-25T10:00:00".into()),
            ..aware.clone()
        };
        for input in [vec![aware.clone(), floating.clone()], vec![floating, aware]] {
            let mut merged = Vec::new();
            for event in input {
                insert(&mut merged, event);
            }
            let saved = parse_ical(&merged[0].to_ical().unwrap());
            assert_eq!(saved[0].end.as_deref(), Some("2027-01-25T09:00:00Z"));
        }
    }
}
