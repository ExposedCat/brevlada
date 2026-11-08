use super::*;
use std::collections::HashMap;

pub(super) fn entries(sources: &[Source], now: i64, range: Range) -> Vec<Entry> {
    let mut events: HashMap<String, Entry> = HashMap::new();
    for source in sources.iter().filter(|source| !source.folder.is_empty()) {
        for event in &source.message.calendar_events {
            if event.method.as_deref() == Some("REPLY") {
                continue;
            }
            let identifier = serde_json::to_string(&(
                event.uid.as_deref().unwrap_or(&event.name),
                &event.recurrence_id,
                &event.organizer_email,
                event.uid.is_none().then_some(&event.start),
            ))
            .unwrap();
            let id = super::collect::key(source, "event", &identifier);
            let newer = events.get(&id).is_none_or(|old| {
                let Content::Event(previous) = &old.content else {
                    unreachable!()
                };
                (event.sequence, event.stamp, source.message.timestamp)
                    > (
                        previous.sequence,
                        previous.stamp,
                        old.source.message.timestamp,
                    )
            });
            if newer {
                events.insert(
                    id.clone(),
                    Entry {
                        recent_emails: 0,
                        id,
                        source: Source {
                            account: source.account.clone(),
                            folder: source.folder.clone(),
                            message: source.message.list_header(false),
                        },
                        content: Content::Event(event.clone()),
                        series: Vec::new(),
                    },
                );
            }
        }
    }
    let organizers = super::event_identity::organizers(sources);
    let mut resolved = Vec::new();
    for mut entry in events.into_values() {
        let Content::Event(event) = &mut entry.content else {
            unreachable!()
        };
        *event = calendar::snapshot(
            event,
            sources
                .iter()
                .filter(|source| source.account == entry.source.account)
                .map(|source| &source.message),
        );
        resolved.push(entry);
    }
    resolved.sort_by_key(|entry| entry.source.message.timestamp);
    let mut aliases = HashMap::new();
    for entry in &resolved {
        let Content::Event(event) = &entry.content else {
            unreachable!()
        };
        if let Some(id) = super::event_identity::calendar_key(&entry.source, event) {
            aliases.insert(
                id,
                super::event_identity::group(&entry.source, event, &organizers),
            );
        }
    }
    let mut groups: HashMap<String, Vec<Entry>> = HashMap::new();
    for entry in resolved {
        let Content::Event(event) = &entry.content else {
            unreachable!()
        };
        let group = super::event_identity::calendar_key(&entry.source, event)
            .and_then(|id| aliases.get(&id).cloned())
            .unwrap_or_else(|| super::event_identity::group(&entry.source, event, &organizers));
        groups.entry(group).or_default().push(entry);
    }
    let future = range.future(now);
    let custom = matches!(range, Range::Custom { .. });
    groups
        .into_iter()
        .filter_map(|(group, entries)| {
            let mut entries = super::event_identity::deduplicate(entries);
            entries.sort_by(|left, right| {
                left.date()
                    .and_then(timestamp)
                    .cmp(&right.date().and_then(timestamp))
                    .then_with(|| left.id.cmp(&right.id))
            });
            let selected = entries.iter().position(|entry| {
                let Content::Event(event) = &entry.content else {
                    unreachable!()
                };
                future.overlaps(
                    &event.start,
                    event.end.as_deref(),
                    event.calendar.is_some() && event.end.is_some(),
                ) && event.method.as_deref() != Some("CANCEL")
                    && !event.status.as_deref().is_some_and(|status| {
                        matches!(
                            status.rsplit('/').next(),
                            Some("CANCELLED" | "EventCancelled")
                        )
                    })
                    && (custom || event_upcoming(event, now))
            })?;
            let recurring = entries.len() > 1
                || entries.iter().any(|entry| {
                    let Content::Event(event) = &entry.content else {
                        unreachable!()
                    };
                    event.recurrence_id.is_some()
                        || event.calendar.as_deref().is_some_and(|calendar| {
                            calendar.lines().any(|line| line.starts_with("RRULE:"))
                        })
                });
            let series = if recurring {
                entries
                    .iter()
                    .map(|entry| {
                        let Content::Event(event) = &entry.content else {
                            unreachable!()
                        };
                        Occurrence {
                            event: event.clone(),
                            source: entry.source.clone(),
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let mut entry = entries.swap_remove(selected);
            if recurring {
                entry.id = group;
                entry.series = series;
            }
            Some(entry)
        })
        .collect()
}
