//! A bounded, latest-request-only queue, independent of mail/network workers.
mod senders;

use crate::models::{self, Message};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub account: String,
    pub folder: String,
}

impl Location {
    pub fn key(&self, sender: &str) -> String {
        // Length-prefix each part, since folder names can contain punctuation.
        format!(
            "{}:{}{}:{}{}",
            self.account.len(),
            self.account,
            self.folder.len(),
            self.folder,
            sender
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub current: Option<Location>,
    /// None searches every cached folder in each selected account.
    pub folder: Option<String>,
    pub accounts: Vec<String>,
    pub cache_path: std::path::PathBuf,
    pub revision: u64,
}

pub struct Request {
    pub threads: bool,
    pub filters: models::search::Filters,
    pub scope: Option<Scope>,
    pub query: String,
    pub sender: Option<String>,
    pub own: Option<String>,
    pub messages: Arc<Vec<Message>>,
    pub sender_messages: Option<Arc<Vec<Message>>>,
    pub unread_first: bool,
    pub deferred_read: HashSet<u32>,
    ticket: u64,
}

impl Request {
    pub fn new(
        threads: bool,
        query: String,
        sender: Option<String>,
        own: Option<String>,
        messages: Arc<Vec<Message>>,
    ) -> Self {
        Self {
            threads,
            filters: models::search::Filters::default(),
            scope: None,
            query,
            sender,
            own,
            messages,
            sender_messages: None,
            unread_first: true,
            deferred_read: HashSet::new(),
            ticket: 0,
        }
    }
}

pub struct Response {
    pub threads: bool,
    pub ticket: u64,
    pub query: String,
    pub groups: Vec<Vec<Message>>,
    pub locations: Vec<Option<Location>>,
    pub error: Option<String>,
    /// Partial responses contain the complete snapshot so far.
    pub complete: bool,
}

#[derive(Default)]
struct Pending {
    requests: [Option<Request>; 2],
    stopped: bool,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Pending>,
    ready: Condvar,
    tickets: [AtomicU64; 2],
}

pub struct Search(Arc<Shared>);

struct Index {
    source: Arc<Vec<Message>>,
    sender_source: Option<Arc<Vec<Message>>>,
    sender: Option<String>,
    messages: Arc<Vec<Message>>,
    fields: Vec<Arc<[String]>>,
    senders: Vec<String>,
}

impl Search {
    pub fn start() -> (Self, async_channel::Receiver<Response>) {
        let shared = Arc::new(Shared::default());
        // Backpressure prevents completed results accumulating on a busy UI.
        let (sender, receiver) = async_channel::bounded(2);
        let worker = shared.clone();
        std::thread::spawn(move || {
            let mut thread_index: Option<Index> = None;
            let mut next_lane = 0;
            let mut sender_cache = senders::Cache::default();
            loop {
                let request = {
                    let mut pending = worker.pending.lock().unwrap();
                    while !pending.stopped && pending.requests.iter().all(Option::is_none) {
                        pending = worker.ready.wait(pending).unwrap();
                    }
                    if pending.stopped {
                        break;
                    }
                    // Each pane gets a turn even while the other is being typed in.
                    let lane = if pending.requests[next_lane].is_some() {
                        next_lane
                    } else {
                        1 - next_lane
                    };
                    next_lane = 1 - lane;
                    pending.requests[lane].take().unwrap()
                };
                if !current(&worker, &request) {
                    continue;
                }
                if !request.threads {
                    if let Err(error) = sender_cache.search(&request, &worker, &sender)
                        && current(&worker, &request)
                    {
                        let _ = sender.send_blocking(Response {
                            threads: false,
                            ticket: request.ticket,
                            query: request.query,
                            groups: Vec::new(),
                            locations: Vec::new(),
                            error: Some(error.to_string()),
                            complete: true,
                        });
                    }
                    continue;
                }
                if !thread_index.as_ref().is_some_and(|index| {
                    Arc::ptr_eq(&index.source, &request.messages)
                        && index.sender == request.sender
                        && match (&index.sender_source, &request.sender_messages) {
                            (None, None) => true,
                            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                            _ => false,
                        }
                }) {
                    let messages: Arc<Vec<Message>> = {
                        let mut scoped = HashMap::new();
                        for message in request.messages.iter().chain(
                            request
                                .sender_messages
                                .iter()
                                .flat_map(|messages| messages.iter()),
                        ) {
                            if !current(&worker, &request) {
                                break;
                            }
                            let sender = models::senders::key(message);
                            if request.sender.as_ref() == Some(&sender)
                                || request.own.as_ref() == Some(&sender)
                            {
                                scoped.insert(
                                    (message.uid, message.message_id.clone()),
                                    message.clone(),
                                );
                            }
                        }
                        Arc::new(scoped.into_values().collect())
                    };
                    let previous: HashMap<_, _> = thread_index
                        .as_ref()
                        .map(|index| {
                            index
                                .messages
                                .iter()
                                .enumerate()
                                .map(|(position, message)| {
                                    (
                                        (message.uid, message.message_id.as_str()),
                                        (index, position),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut fields = Vec::with_capacity(messages.len());
                    let mut senders = Vec::with_capacity(messages.len());
                    for message in messages.iter() {
                        if !current(&worker, &request) {
                            break;
                        }
                        if let Some(&(old, position)) =
                            previous.get(&(message.uid, message.message_id.as_str()))
                            && same_fields(message, &old.messages[position])
                        {
                            fields.push(old.fields[position].clone());
                            senders.push(old.senders[position].clone());
                        } else {
                            fields.push(
                                models::search::fields(message)
                                    .iter()
                                    .map(|field| field.to_lowercase())
                                    .collect::<Vec<_>>()
                                    .into(),
                            );
                            senders.push(models::senders::key(message));
                        }
                    }
                    if !current(&worker, &request) {
                        continue;
                    }
                    thread_index = Some(Index {
                        source: request.messages.clone(),
                        sender_source: request.sender_messages.clone(),
                        sender: request.sender.clone(),
                        messages,
                        fields,
                        senders,
                    });
                }
                let index = thread_index.as_ref().unwrap();
                if let Some(mut groups) = search(index, &request, || current(&worker, &request)) {
                    groups.sort_by_key(|(location, group)| {
                        std::cmp::Reverse((
                            request.unread_first
                                && group.iter().any(|message| {
                                    !message.is_read
                                        || (location.as_ref().is_none_or(|location| {
                                            request.scope.as_ref().is_some_and(|scope| {
                                                Some(location) == scope.current.as_ref()
                                            })
                                        }) && request.deferred_read.contains(&message.uid))
                                }),
                            group[0].timestamp,
                            group[0].uid,
                        ))
                    });
                    let (locations, groups) = groups.into_iter().unzip();
                    if current(&worker, &request)
                        && sender
                            .send_blocking(Response {
                                threads: request.threads,
                                ticket: request.ticket,
                                query: request.query,
                                groups,
                                locations,
                                error: None,
                                complete: true,
                            })
                            .is_err()
                    {
                        break;
                    }
                }
            }
        });
        (Self(shared), receiver)
    }

    pub fn submit(&self, mut request: Request) {
        let lane = usize::from(request.threads);
        request.ticket = self.0.tickets[lane].fetch_add(1, Ordering::Relaxed) + 1;
        self.0.pending.lock().unwrap().requests[lane] = Some(request);
        self.0.ready.notify_one();
    }

    pub fn cancel(&self, threads: bool) {
        let lane = usize::from(threads);
        self.0.tickets[lane].fetch_add(1, Ordering::Relaxed);
        self.0.pending.lock().unwrap().requests[lane] = None;
    }

    pub fn accepts(&self, response: &Response) -> bool {
        self.0.tickets[usize::from(response.threads)].load(Ordering::Relaxed) == response.ticket
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        for ticket in &self.0.tickets {
            ticket.fetch_add(1, Ordering::Relaxed);
        }
        self.0.pending.lock().unwrap().stopped = true;
        self.0.ready.notify_one();
    }
}

fn same_fields(a: &Message, b: &Message) -> bool {
    a.subject == b.subject
        && a.sender == b.sender
        && a.reply_to == b.reply_to
        && a.recipients == b.recipients
        && a.cc == b.cc
        && a.body_text == b.body_text
        && a.body_html == b.body_html
}

fn current(shared: &Shared, request: &Request) -> bool {
    shared.tickets[usize::from(request.threads)].load(Ordering::Relaxed) == request.ticket
}

fn search(
    index: &Index,
    request: &Request,
    current: impl Fn() -> bool,
) -> Option<Vec<(Option<Location>, Vec<Message>)>> {
    let mut messages = Vec::new();
    for (position, message) in index.messages.iter().enumerate() {
        if !current() {
            return None;
        }
        let sender = &index.senders[position];
        if (request.sender.as_ref() == Some(sender) || request.own.as_ref() == Some(sender))
            && request.filters.includes_date(message.timestamp)
            && request
                .filters
                .matches_fields(&index.fields[position], &request.query)
        {
            let mut message = message.clone();
            message.prepare_display();
            messages.push(message.list_header(true));
        }
    }
    let mut groups = models::threads(&messages, "");
    groups.retain(|group| {
        group
            .iter()
            .any(|message| Some(models::senders::key(message)) == request.sender)
    });
    Some(groups.into_iter().map(|group| (None, group)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn finished(results: &async_channel::Receiver<Response>) -> Response {
        loop {
            let response = results.recv_blocking().unwrap();
            if response.complete {
                return response;
            }
        }
    }

    fn messages() -> Arc<Vec<Message>> {
        Arc::new(vec![
            Message {
                uid: 1,
                sender: "Alice <alice@example.com>".into(),
                recipients: "Receiver <receiver@example.com>".into(),
                subject: "Old".into(),
                body_text: "Needle".into(),
                ..Default::default()
            },
            Message {
                uid: 2,
                timestamp: 2,
                sender: "alice@example.com".into(),
                subject: "Latest".into(),
                ..Default::default()
            },
            Message {
                uid: 3,
                sender: "bob@example.com".into(),
                body_text: "Needle".into(),
                ..Default::default()
            },
        ])
    }

    #[test]
    fn changing_sources_and_dates_reuses_index_without_reusing_results() {
        let (worker, results) = Search::start();
        let messages = messages();
        let mut request = Request::new(false, "needle".into(), None, None, messages.clone());
        request.filters.sources[5] = false;
        worker.submit(request);
        assert!(finished(&results).groups.is_empty());
        let mut request = Request::new(false, "needle".into(), None, None, messages.clone());
        request.filters.from = chrono::NaiveDate::from_ymd_opt(2026, 10, 2);
        worker.submit(request);
        assert!(finished(&results).groups.is_empty());
        worker.submit(Request::new(false, "needle".into(), None, None, messages));
        assert_eq!(finished(&results).groups.len(), 2);
    }

    #[test]
    fn account_scope_keeps_overlapping_uids_separate_and_refreshes_other_account_cache() {
        let path = std::env::temp_dir().join(format!(
            "brevlada-search-{}-{}.db",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let storage = super::super::storage::Storage::open(&path).unwrap();
        let original = Message {
            uid: 1,
            sender: "shared@example.com".into(),
            body_text: "Current needle".into(),
            ..Default::default()
        };
        let other = Message {
            body_text: "Other needle".into(),
            ..original.clone()
        };
        storage.store("a", "INBOX", &original).unwrap();
        storage.store("b", "INBOX", &other).unwrap();
        storage
            .store(
                "b",
                "Archive",
                &Message {
                    body_text: "Wrong folder".into(),
                    ..other.clone()
                },
            )
            .unwrap();
        let messages = Arc::new(vec![original]);
        let (worker, results) = Search::start();
        let mut scope = Scope {
            current: Some(Location {
                account: "a".into(),
                folder: "INBOX".into(),
            }),
            folder: Some("INBOX".into()),
            accounts: vec!["a".into(), "b".into()],
            cache_path: path.clone(),
            revision: 0,
        };
        let run = |query: &str, scope: &Scope| {
            let mut request = Request::new(false, query.into(), None, None, messages.clone());
            request.scope = Some(scope.clone());
            worker.submit(request);
            let response = finished(&results);
            assert!(response.error.is_none(), "{:?}", response.error);
            response
        };
        let result = run("needle", &scope);
        assert_eq!(result.groups.len(), 2);
        let accounts: HashSet<_> = result
            .locations
            .into_iter()
            .map(|location| location.unwrap().account)
            .collect();
        assert_eq!(accounts, HashSet::from(["a".to_string(), "b".to_string()]));
        let result = run("other needle", &scope);
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.locations[0].as_ref().unwrap().account, "b");
        assert!(run("wrong folder", &scope).groups.is_empty());
        // All Folders includes Archive with the same UID and sender as INBOX.
        scope.folder = None;
        let archive = run("wrong folder", &scope);
        assert_eq!(archive.groups.len(), 1);
        assert_eq!(archive.locations[0].as_ref().unwrap().account, "b");
        assert_eq!(archive.locations[0].as_ref().unwrap().folder, "Archive");
        assert_eq!(run("", &scope).groups.len(), 3);
        // Current Folder uses the exact same path across accounts. Account a
        // has no Archive; it must not fall back to its INBOX.
        scope.folder = Some("Archive".into());
        assert_eq!(run("", &scope).groups.len(), 1);
        assert!(run("current needle", &scope).groups.is_empty());
        scope.current = None;
        scope.folder = None;
        assert_eq!(run("", &scope).groups.len(), 3);
        scope.folder = Some("Missing".into());
        assert!(run("", &scope).groups.is_empty());
        scope.folder = Some("INBOX".into());
        scope.accounts = vec!["b".into()];
        assert_eq!(run("other needle", &scope).groups.len(), 1);
        assert!(run("current needle", &scope).groups.is_empty());
        scope.current = None;
        scope.accounts = vec!["a".into(), "b".into()];
        assert_eq!(run("needle", &scope).groups.len(), 2);
        scope.accounts = vec!["b".into()];
        assert_eq!(run("other needle", &scope).groups.len(), 1);
        assert!(run("current needle", &scope).groups.is_empty());
        storage
            .store(
                "b",
                "INBOX",
                &Message {
                    body_text: "New cached body".into(),
                    ..other
                },
            )
            .unwrap();
        // Detect cache writes even if no refresh event arrived while deselected.
        assert_eq!(run("new cached body", &scope).groups.len(), 1);
        scope.accounts.clear();
        assert!(run("", &scope).groups.is_empty());
        drop(storage);
        drop(worker);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sender_search_previews_the_match_and_message_search_stays_in_sender() {
        let (worker, results) = Search::start();
        let messages = messages();
        worker.submit(Request::new(
            false,
            "receiver".into(),
            None,
            None,
            messages.clone(),
        ));
        let result = finished(&results);
        assert!(worker.accepts(&result));
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0][0].uid, 1);
        assert!(result.groups[0][0].search_match);
        assert_eq!(models::senders::preview(&result.groups[0]).unwrap().uid, 1);
        worker.submit(Request::new(
            true,
            "needle".into(),
            Some("alice@example.com".into()),
            None,
            messages,
        ));
        let result = finished(&results);
        assert_eq!(
            result
                .groups
                .iter()
                .flatten()
                .map(|message| message.uid)
                .collect::<Vec<_>>(),
            [1]
        );
    }

    #[test]
    fn superseded_and_cleared_queries_cannot_publish_old_results() {
        let (worker, results) = Search::start();
        worker.submit(Request::new(false, "needle".into(), None, None, messages()));
        let old = finished(&results);
        worker.submit(Request::new(
            false,
            "missing".into(),
            None,
            None,
            messages(),
        ));
        assert!(!worker.accepts(&old));
        let latest = finished(&results);
        assert!(latest.groups.is_empty());
        worker.cancel(false);
        assert!(!worker.accepts(&latest));
    }
    #[test]
    fn refreshing_snapshot_and_sender_page_replaces_searchable_fields_without_duplicates() {
        let (worker, results) = Search::start();
        let original = messages();
        worker.submit(Request::new(
            false,
            "needle".into(),
            None,
            None,
            original.clone(),
        ));
        assert_eq!(finished(&results).groups.len(), 2);
        let mut updated = original.as_ref().clone();
        updated[0].body_text = "Changed body".into();
        updated[0].cc = "New recipient".into();
        let updated = Arc::new(updated);
        worker.submit(Request::new(
            false,
            "needle".into(),
            None,
            None,
            updated.clone(),
        ));
        let result = finished(&results);
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0][0].uid, 3);
        let mut request = Request::new(
            true,
            "new recipient".into(),
            Some("alice@example.com".into()),
            None,
            original,
        );
        request.sender_messages = Some(updated);
        worker.submit(request);
        let result = finished(&results);
        assert_eq!(
            result
                .groups
                .iter()
                .flatten()
                .map(|message| message.uid)
                .collect::<Vec<_>>(),
            [1]
        );
    }

    #[test]
    #[ignore = "Synthetic large-folder performance diagnostic"]
    fn large_folder_search_latency() {
        let messages = Arc::new(
            (1..=20_000)
                .map(|uid| Message {
                    uid,
                    timestamp: uid as i64,
                    sender: format!("sender{}@example.com", uid % 2_000),
                    body_text: "Long cached message contents. ".repeat(128),
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
        );
        let (worker, results) = Search::start();
        for query in ["missing", "cached message", "sender123@", "another missing"] {
            let started = std::time::Instant::now();
            worker.submit(Request::new(
                false,
                query.into(),
                None,
                None,
                messages.clone(),
            ));
            let result = finished(&results);
            assert!(worker.accepts(&result));
            eprintln!(
                "20,000 messages: {query:?}: {:?}, {} senders",
                started.elapsed(),
                result.groups.len()
            );
        }
    }
}
