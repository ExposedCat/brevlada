use super::*;
use crate::models::senders;
use std::collections::{HashMap, HashSet};

pub(super) type Organizers = HashMap<(String, String, String), HashSet<String>>;

fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn owner(event: &calendar::Event) -> Option<String> {
    event
        .organizer
        .as_deref()
        .or(event.organizer_email.as_deref())
        .map(normalized)
        .filter(|owner| !owner.is_empty())
}

fn source_key(source: &Source, event: &calendar::Event) -> (String, String, String) {
    (
        source.account.clone(),
        normalized(&event.name),
        senders::key(&source.message),
    )
}

pub(super) fn organizers(sources: &[Source]) -> Organizers {
    let mut owners: Organizers = HashMap::new();
    for source in sources {
        for event in &source.message.calendar_events {
            if event.method.as_deref() != Some("REPLY")
                && let Some(owner) = owner(event)
            {
                owners
                    .entry(source_key(source, event))
                    .or_default()
                    .insert(owner);
            }
        }
    }
    owners
}

pub(super) fn group(source: &Source, event: &calendar::Event, organizers: &Organizers) -> String {
    let owner = owner(event)
        .or_else(|| {
            organizers
                .get(&source_key(source, event))
                .filter(|owners| owners.len() == 1)
                .and_then(|owners| owners.iter().next().cloned())
        })
        .unwrap_or_else(|| senders::key(&source.message));
    serde_json::to_string(&(
        &source.account,
        "event-series",
        normalized(&event.name),
        owner,
    ))
    .unwrap()
}

pub(super) fn calendar_key(source: &Source, event: &calendar::Event) -> Option<String> {
    event
        .uid
        .as_ref()
        .map(|uid| serde_json::to_string(&(&source.account, uid, &event.organizer_email)).unwrap())
}

pub(super) fn deduplicate(entries: Vec<Entry>) -> Vec<Entry> {
    let mut result: Vec<Entry> = Vec::new();
    for entry in entries {
        let Content::Event(event) = &entry.content else {
            unreachable!()
        };
        let rank = |event: &calendar::Event, source: &Source| {
            (
                event.calendar.is_some(),
                event.uid.is_some(),
                event.sequence,
                event.stamp,
                source.message.timestamp,
            )
        };
        let duplicate = result.iter_mut().find(|old| {
            let Content::Event(previous) = &old.content else {
                unreachable!()
            };
            match (
                DateTime::parse_from_rfc3339(&previous.start),
                DateTime::parse_from_rfc3339(&event.start),
            ) {
                (Ok(previous), Ok(current)) => previous == current,
                _ => previous.start == event.start && previous.timezone == event.timezone,
            }
        });
        if let Some(old) = duplicate {
            let Content::Event(previous) = &old.content else {
                unreachable!()
            };
            if rank(event, &entry.source) > rank(previous, &old.source) {
                *old = entry;
            }
        } else {
            result.push(entry);
        }
    }
    result
}
