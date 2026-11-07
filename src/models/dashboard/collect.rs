use super::*;
use crate::models::senders;
use std::collections::HashSet;

pub fn entries(mut sources: Vec<Source>, now: i64, range: Range) -> Vec<Entry> {
    let past = range.past(now);
    let future = range.future(now);
    let custom = matches!(range, Range::Custom { .. });
    sources.sort_by_key(|source| std::cmp::Reverse(source.message.timestamp));
    let mut seen = HashSet::new();
    let mut result = super::parcels::entries(&sources, now, range);
    result.extend(super::subscriptions::entries(&sources));
    result.extend(super::events::entries(&sources, now, range));
    for source in &sources {
        if source.folder.is_empty() {
            continue;
        }
        let message = &source.message;
        let sender = senders::key(message);
        for ticket in &message.tickets {
            let identifier = serde_json::to_string(&(
                format!("{:?}", ticket.kind),
                &sender,
                &ticket.number,
                &ticket.name,
                &ticket.start,
                &ticket.departure,
            ))
            .unwrap();
            let id = key(source, "ticket", &identifier);
            if !seen.insert(id.clone())
                || ticket.status.as_deref() == Some("ReservationCancelled")
                || !ticket
                    .start
                    .as_deref()
                    .or(ticket.end.as_deref())
                    .map(|start| future.overlaps(start, ticket.end.as_deref(), false))
                    .unwrap_or(false)
                || !ticket
                    .end
                    .as_deref()
                    .or(ticket.start.as_deref())
                    .map(|date| custom || upcoming(date, now))
                    .unwrap_or(true)
            {
                continue;
            }
            result.push(entry(source, id, Content::Ticket(ticket.clone())));
        }
        for receipt in &message.receipts {
            if !receipt
                .date
                .as_deref()
                .map(|date| past.overlaps(date, None, false))
                .unwrap_or(false)
            {
                continue;
            }
            let id = key(
                source,
                "receipt",
                &format!("{sender}:{}:{}", receipt.invoice, receipt.number),
            );
            if seen.insert(id.clone()) {
                result.push(entry(source, id, Content::Receipt(receipt.clone())));
            }
        }
    }
    result.sort_by(|left, right| {
        left.section()
            .cmp(&right.section())
            .then_with(|| {
                if matches!(left.section(), Section::Receipts | Section::Orders) {
                    right
                        .date()
                        .and_then(timestamp)
                        .cmp(&left.date().and_then(timestamp))
                } else {
                    left.date()
                        .and_then(timestamp)
                        .unwrap_or(i64::MAX)
                        .cmp(&right.date().and_then(timestamp).unwrap_or(i64::MAX))
                }
            })
            .then_with(|| left.id.cmp(&right.id))
    });
    result
}

pub(super) fn key(source: &Source, kind: &str, identifier: &str) -> String {
    serde_json::to_string(&(&source.account, kind, identifier)).unwrap()
}

fn entry(source: &Source, id: String, content: Content) -> Entry {
    Entry {
        id,
        source: Source {
            account: source.account.clone(),
            folder: source.folder.clone(),
            message: source.message.list_header(false),
        },
        content,
        series: Vec::new(),
    }
}
