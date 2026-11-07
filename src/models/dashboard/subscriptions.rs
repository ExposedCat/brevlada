use super::{Content, Entry, Source, collect::key};
use crate::models::senders;
use std::collections::HashSet;

pub(super) fn entries(sources: &[Source]) -> Vec<Entry> {
    let mut seen = HashSet::new();
    sources
        .iter()
        .filter(|source| !source.folder.is_empty() && source.message.unsubscribe.is_some())
        .filter_map(|source| {
            let id = key(source, "subscription", &senders::key(&source.message));
            seen.insert(id.clone()).then(|| Entry {
                id,
                source: Source {
                    account: source.account.clone(),
                    folder: source.folder.clone(),
                    message: crate::models::Message {
                        unsubscribe: source.message.unsubscribe.clone(),
                        ..source.message.list_header(false)
                    },
                },
                content: Content::Subscription,
                series: Vec::new(),
            })
        })
        .collect()
}
