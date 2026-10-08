//! Reusable per-mailbox fields, lazy HTML decoding, and incremental sender results.
use super::*;
use std::{
    cell::OnceCell,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Cache {
    path: Option<std::path::PathBuf>,
    storage: Option<super::super::storage::Storage>,
    mailboxes: HashMap<Option<Location>, Mailbox>,
}

#[derive(Default)]
struct Mailbox {
    source: Option<Source>,
    messages: HashMap<u32, Entry>,
}

enum Source {
    Memory(Arc<Vec<Message>>),
    Disk(i64, u64),
}

struct Entry {
    message: Message,
    sender: String,
    fields: [OnceCell<String>; 7],
}

impl Entry {
    fn new(message: Message) -> Self {
        Self {
            sender: models::senders::key(&message),
            message,
            fields: std::array::from_fn(|_| OnceCell::new()),
        }
    }

    fn matches(&self, request: &Request) -> bool {
        if !request.filters.includes_date(self.message.timestamp) {
            return false;
        }
        if request.query.is_empty() {
            return true;
        }
        let message = &self.message;
        let raw = [
            &message.subject,
            &message.sender,
            &message.reply_to,
            &message.recipients,
            &message.cc,
            &message.body_text,
            &message.body_html,
        ];
        (0..7).any(|index| {
            request.filters.sources[index.min(5)]
                && self.fields[index]
                    .get_or_init(|| {
                        if index == 6 && !raw[index].is_empty() {
                            models::search::html_text(raw[index]).to_lowercase()
                        } else {
                            raw[index].to_lowercase()
                        }
                    })
                    .contains(&request.query)
        })
    }

    // Sender rows and actions need identities and flags, not copies of every body.
    fn header(&self) -> Message {
        self.message.list_header(false)
    }
}

struct Progress<'a> {
    request: &'a Request,
    worker: &'a Shared,
    output: &'a async_channel::Sender<Response>,
    groups: HashMap<(Option<Location>, String), Vec<Message>>,
    last_sent: Option<Instant>,
    dirty: bool,
}

impl Progress<'_> {
    fn visit(&mut self, location: &Option<Location>, entry: &Entry) -> bool {
        if !current(self.worker, self.request) {
            return false;
        }
        if !entry.matches(self.request) {
            return true;
        }
        let group = self
            .groups
            .entry((location.clone(), entry.sender.clone()))
            .or_default();
        let mut header = entry.header();
        header.search_match = true;
        group.push(header);
        self.dirty = true;
        if self.dirty
            && self
                .last_sent
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(60))
        {
            return self.publish(false);
        }
        true
    }

    fn publish(&mut self, complete: bool) -> bool {
        if !current(self.worker, self.request) {
            return false;
        }
        let mut groups: Vec<_> = self
            .groups
            .iter_mut()
            .map(|((location, _), group)| {
                group.sort_by_key(|m| std::cmp::Reverse((m.timestamp, m.uid)));
                (location.clone(), group.clone())
            })
            .collect();
        groups.sort_by_key(|(location, group)| {
            std::cmp::Reverse((
                self.request.unread_first
                    && group.iter().any(|m| {
                        !m.is_read
                            || (location.as_ref().is_none_or(|location| {
                                self.request
                                    .scope
                                    .as_ref()
                                    .is_some_and(|scope| Some(location) == scope.current.as_ref())
                            }) && self.request.deferred_read.contains(&m.uid))
                    }),
                group[0].timestamp,
                group[0].uid,
            ))
        });
        let (locations, groups) = groups.into_iter().unzip();
        self.last_sent = Some(Instant::now());
        self.dirty = false;
        self.output
            .send_blocking(Response {
                threads: false,
                ticket: self.request.ticket,
                query: self.request.query.clone(),
                groups,
                locations,
                error: None,
                complete,
            })
            .is_ok()
    }
}

impl Cache {
    pub(super) fn search(
        &mut self,
        request: &Request,
        worker: &Shared,
        output: &async_channel::Sender<Response>,
    ) -> anyhow::Result<()> {
        let mut progress = Progress {
            request,
            worker,
            output,
            groups: HashMap::new(),
            last_sent: None,
            dirty: false,
        };
        if let Some(scope) = &request.scope {
            if self.path.as_ref() != Some(&scope.cache_path) {
                self.mailboxes.clear();
                self.storage = None;
                self.path = Some(scope.cache_path.clone());
            }
            // Reuse prepared fields when account selections change.
            self.mailboxes.retain(|location, _| {
                location.as_ref().is_some_and(|location| {
                    scope
                        .folder
                        .as_ref()
                        .is_none_or(|folder| &location.folder == folder)
                })
            });
            if self.storage.is_none()
                && scope.cache_path.exists()
                && !scope.accounts.is_empty()
                && (scope.folder.is_none()
                    || scope.accounts.iter().any(|account| {
                        scope.current.as_ref().is_none_or(|current| {
                            current.account != *account
                                || Some(&current.folder) != scope.folder.as_ref()
                        })
                    }))
            {
                self.storage = Some(super::super::storage::Storage::open_read_only(
                    &scope.cache_path,
                )?);
            }
        }
        let mut locations = Vec::new();
        if let Some(scope) = &request.scope {
            for account in &scope.accounts {
                let mut folders = if let Some(folder) = &scope.folder {
                    vec![folder.clone()]
                } else {
                    self.storage
                        .as_ref()
                        .map(|storage| storage.search_folders(account))
                        .transpose()?
                        .unwrap_or_default()
                };
                // Include the active mailbox even before its first cache write.
                if scope.folder.is_none()
                    && let Some(current) = &scope.current
                    && current.account == *account
                    && !folders.contains(&current.folder)
                {
                    folders.push(current.folder.clone());
                }
                locations.extend(folders.into_iter().map(|folder| {
                    Some(Location {
                        account: account.clone(),
                        folder,
                    })
                }));
            }
        } else {
            locations.push(None);
        }
        // Keep one connection: data_version detects writes even while search is closed
        // or an account is deselected. Unchanged messages retain their prepared fields.
        let version = self
            .storage
            .as_ref()
            .map(|storage| storage.search_data_version())
            .transpose()?
            .unwrap_or(0);
        for location in locations {
            if !current(worker, request) {
                return Ok(());
            }
            let memory = request
                .scope
                .as_ref()
                .is_none_or(|scope| scope.current == location);
            let revision = request.scope.as_ref().map_or(0, |scope| scope.revision);
            let mailbox = self.mailboxes.entry(location.clone()).or_default();
            let reusable = match &mailbox.source {
                Some(Source::Memory(source)) => memory && Arc::ptr_eq(source, &request.messages),
                Some(Source::Disk(old, rev)) => !memory && *old == version && *rev == revision,
                None => false,
            };
            if reusable {
                // Newest messages first also keeps partial previews stable.
                let mut entries: Vec<_> = mailbox.messages.values().collect();
                entries.sort_by_key(|entry| {
                    std::cmp::Reverse((entry.message.timestamp, entry.message.uid))
                });
                for entry in entries {
                    if !progress.visit(&location, entry) {
                        return Ok(());
                    }
                }
                continue;
            }
            mailbox.source = None;
            let mut seen = HashSet::new();
            let mut visit = |message: Message| {
                if !current(worker, request) {
                    return false;
                }
                seen.insert(message.uid);
                let entry = mailbox
                    .messages
                    .entry(message.uid)
                    .or_insert_with(|| Entry::new(message.clone()));
                if !same_fields(&entry.message, &message) {
                    *entry = Entry::new(message);
                } else {
                    entry.message = message;
                }
                progress.visit(&location, entry)
            };
            if memory {
                let mut messages: Vec<_> = request.messages.iter().collect();
                messages.sort_by_key(|m| std::cmp::Reverse((m.timestamp, m.uid)));
                for message in messages {
                    if !visit(message.clone()) {
                        return Ok(());
                    }
                }
            } else if let Some(location) = &location
                && let Some(storage) = &self.storage
            {
                storage.visit_search_messages(&location.account, &location.folder, &mut visit)?;
            }
            if !current(worker, request) {
                return Ok(());
            }
            mailbox.messages.retain(|uid, _| seen.contains(uid));
            mailbox.source = Some(if memory {
                Source::Memory(request.messages.clone())
            } else {
                Source::Disk(version, revision)
            });
        }
        progress.publish(true);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_the_newest_matching_message_even_when_nonmatches_are_unread() {
        let messages = Arc::new(vec![
            Message {
                uid: 4,
                timestamp: 4,
                sender: "sender@example.com".into(),
                subject: "Unrelated".into(),
                ..Default::default()
            },
            Message {
                uid: 3,
                timestamp: 3,
                sender: "sender@example.com".into(),
                subject: "Needle newest".into(),
                is_read: true,
                ..Default::default()
            },
            Message {
                uid: 2,
                timestamp: 2,
                sender: "sender@example.com".into(),
                subject: "Needle older".into(),
                ..Default::default()
            },
        ]);
        let (worker, results) = Search::start();
        for query in ["needle", "older", "needle"] {
            worker.submit(Request::new(
                false,
                query.into(),
                None,
                None,
                messages.clone(),
            ));
            let result = super::super::tests::finished(&results);
            let group = &result.groups[0];
            assert_eq!(group.len(), if query == "older" { 1 } else { 2 });
            let expected = if query == "older" { 2 } else { 3 };
            assert_eq!(group[0].uid, expected);
            assert_eq!(models::senders::preview(group).unwrap().uid, expected);
            assert_eq!(
                group.iter().filter(|message| message.search_match).count(),
                group.len()
            );
        }
    }

    #[test]
    fn header_matches_and_disabled_body_do_not_decode_html() {
        let entry = Entry::new(Message {
            subject: "Delivery".into(),
            body_html: "<style>hidden</style><p>Visible &amp; searchable</p>".into(),
            ..Default::default()
        });
        let mut request = Request::new(false, "delivery".into(), None, None, Arc::default());
        assert!(entry.matches(&request));
        assert!(entry.fields[6].get().is_none());
        request.query = "visible & searchable".into();
        request.filters.sources[5] = false;
        assert!(!entry.matches(&request));
        assert!(entry.fields[6].get().is_none());
        request.filters.sources[5] = true;
        assert!(entry.matches(&request));
        request.query = "hidden".into();
        assert!(!entry.matches(&request));
    }

    #[test]
    fn streams_partial_results_and_finishes_with_matching_previews() {
        let messages = Arc::new(
            (1..=256)
                .map(|uid| Message {
                    uid,
                    timestamp: uid as i64,
                    sender: format!("sender{}@example.com", uid % 32),
                    subject: if uid <= 32 {
                        "needle".into()
                    } else {
                        "Latest".into()
                    },
                    body_html: "<p>Large body</p>".repeat(64),
                    ..Default::default()
                })
                .collect(),
        );
        let (worker, results) = Search::start();
        worker.submit(Request::new(false, "needle".into(), None, None, messages));
        let first = results.recv_blocking().unwrap();
        assert!(!first.complete);
        assert_eq!(first.groups.len(), 1);
        assert!(first.groups[0][0].uid <= 32);
        assert!(first.groups[0][0].search_match);
        assert!(
            first
                .groups
                .iter()
                .flatten()
                .all(|m| m.body_html.is_empty())
        );
        let final_result = super::super::tests::finished(&results);
        assert!(worker.accepts(&final_result));
        assert_eq!(final_result.groups.len(), 32);
        assert!(
            final_result
                .groups
                .iter()
                .all(|group| group.len() == 1 && group[0].uid <= 32 && group[0].search_match)
        );
        worker.submit(Request::new(
            false,
            "missing".into(),
            None,
            None,
            Arc::default(),
        ));
        let empty = results.recv_blocking().unwrap();
        assert!(empty.complete);
        assert!(empty.groups.is_empty());
    }

    #[test]
    #[ignore = "Synthetic cached HTML search performance diagnostic"]
    fn cached_html_search_latency() {
        let path =
            std::env::temp_dir().join(format!("brevlada-html-search-{}.db", std::process::id()));
        let storage = super::super::super::storage::Storage::open(&path).unwrap();
        for uid in 1..=4000 {
            storage
                .store(
                    "a",
                    "INBOX",
                    &Message {
                        uid,
                        timestamp: uid as i64,
                        sender: format!("sender{}@example.com", uid % 400),
                        body_html: "<p>Delivery details and cached content</p>".repeat(64),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let baseline = Instant::now();
        let old = storage.sender_headers("a", "INBOX").unwrap();
        let _fields: Vec<_> = old
            .iter()
            .map(|message| {
                models::search::fields(message)
                    .iter()
                    .map(|field| field.to_lowercase())
                    .collect::<Vec<_>>()
            })
            .collect();
        eprintln!(
            "Old cached HTML preparation (4,000 messages): {:?}",
            baseline.elapsed()
        );
        let (worker, results) = Search::start();
        for query in ["delivery", "delivery details", "absent text", "absent text"] {
            let mut request = Request::new(false, query.into(), None, None, Arc::default());
            request.scope = Some(Scope {
                current: None,
                folder: Some("INBOX".into()),
                accounts: vec!["a".into()],
                cache_path: path.clone(),
                revision: 0,
            });
            let started = Instant::now();
            worker.submit(request);
            let first = results.recv_blocking().unwrap();
            let first_time = started.elapsed();
            let final_result = if first.complete {
                first
            } else {
                super::super::tests::finished(&results)
            };
            eprintln!(
                "{query:?}: first {:?}, complete {:?}, {} senders",
                first_time,
                started.elapsed(),
                final_result.groups.len()
            );
        }
        drop(worker);
        drop(storage);
        let _ = std::fs::remove_file(path);
    }
}
