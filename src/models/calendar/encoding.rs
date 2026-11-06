use super::{Event, ical};

fn byte(ch: char) -> Option<u8> {
    if (ch as u32) <= 255 {
        return Some(ch as u8);
    }
    "€\u{81}‚ƒ„…†‡ˆ‰Š‹Œ\u{8d}Ž\u{8f}\u{90}‘’“”•–—˜™š›œ\u{9d}žŸ"
        .chars()
        .position(|candidate| candidate == ch)
        .map(|index| index as u8 + 128)
}

fn text(value: &str) -> String {
    let chars: Vec<_> = value.chars().collect();
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        let size = match byte(chars[index]) {
            Some(0xc2..=0xdf) => 2,
            Some(0xe0..=0xef) => 3,
            Some(0xf0..=0xf4) => 4,
            _ => 0,
        };
        if size > 0 && index + size <= chars.len() {
            let bytes: Option<Vec<_>> = chars[index..index + size]
                .iter()
                .copied()
                .map(byte)
                .collect();
            if let Some(bytes) = bytes
                && let Ok(decoded) = std::str::from_utf8(&bytes)
            {
                result.push_str(decoded);
                index += size;
                continue;
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

pub(crate) fn repair(event: &mut Event) -> bool {
    let before = event.clone();
    event.name = text(&event.name);
    for value in [
        &mut event.location,
        &mut event.organizer,
        &mut event.description,
    ]
    .into_iter()
    .flatten()
    {
        *value = text(value);
    }
    for attendee in &mut event.attendees {
        if let Some(name) = &mut attendee.name {
            *name = text(name);
        }
    }
    for action in &mut event.rsvp {
        action.label = text(&action.label);
    }
    if let Some(calendar) = &mut event.calendar {
        let mut changed = false;
        let mut result = String::new();
        for line in ical::unfold(calendar) {
            let repaired = match ical::property(&line) {
                Some((key, head, value)) => match key.to_ascii_uppercase().as_str() {
                    "SUMMARY" | "LOCATION" | "DESCRIPTION" | "COMMENT" | "CONTACT"
                    | "RESOURCES" | "CATEGORIES" => format!("{head}:{}", text(value)),
                    "ORGANIZER" | "ATTENDEE" => format!("{}:{value}", text(head)),
                    _ => line.clone(),
                },
                None => line.clone(),
            };
            changed |= repaired != line;
            ical::fold(&repaired, &mut result);
        }
        if changed {
            *calendar = result;
        }
    }
    *event != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_mixed_cached_text_without_changing_identifiers_or_correct_unicode() {
        let broken = "DoruÄ\u{8d}ovÃ¡nÃ\u{ad} · KoÅ¡Ã\u{ad}k.cz · BÅ™evnov · Кот 🐈";
        let mut event = Event {
            name: broken.into(),
            uid: Some("id-Ã©".into()),
            url: Some("https://example.com/Ã©".into()),
            calendar: Some(format!(
                "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:id-Ã©\r\nSUMMARY:{broken}\r\nDTSTART:20261105T120000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            )),
            ..Default::default()
        };
        assert!(repair(&mut event));
        assert_eq!(event.name, "Doručování · Košík.cz · Břevnov · Кот 🐈");
        assert_eq!(event.uid.as_deref(), Some("id-Ã©"));
        assert_eq!(event.url.as_deref(), Some("https://example.com/Ã©"));
        let restored = super::super::parse_ical(event.calendar.as_deref().unwrap());
        assert_eq!(restored[0].name, event.name);
        assert_eq!(restored[0].uid, event.uid);
        assert!(!repair(&mut event));
    }
}
