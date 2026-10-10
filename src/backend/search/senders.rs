//! Reusable per-mailbox fields, lazy HTML decoding, and incremental sender results.
use super::{pairs::Pairs, *};
use std::{
    cell::OnceCell,
    rc::Rc,
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
    /// Newest first once settled, which also keeps partial previews stable.
    entries: Vec<Rc<Entry>>,
}

enum Source {
    Memory(Arc<Vec<Message>>),
    Disk(i64, u64),
}

struct Entry {
    message: Message,
    sender: String,
    fields: [OnceCell<String>; 7],
    pairs: OnceCell<Pairs>,
}

impl Entry {
    fn new(message: Message) -> Self {
        Self {
            sender: models::senders::key(&message),
            message,
            fields: std::array::from_fn(|_| OnceCell::new()),
            pairs: OnceCell::new(),
        }
    }

    fn matches(&self, request: &Request, query: &Pairs) -> bool {
        if !request.filters.includes_date(self.message.timestamp) {
            return false;
        }
        if request.query.is_empty() {
            return true;
        }
        // Summarizing HTML would decode it for every message, so only plain entries skip early.
        if self.message.body_html.is_empty()
            && !self
                .pairs
                .get_or_init(|| Pairs::of((0..6).map(|index| self.field(index))))
                .covers(query)
        {
            return false;
        }
        (0..7).any(|index| {
            request.filters.sources[index.min(5)] && self.field(index).contains(&request.query)
        })
    }

    fn field(&self, index: usize) -> &str {
        self.fields[index].get_or_init(|| {
            let message = &self.message;
            let raw = [
                &message.subject,
                &message.sender,
                &message.reply_to,
                &message.recipients,
                &message.cc,
                &message.body_text,
                &message.body_html,
            ][index];
            if index == 6 && !raw.is_empty() {
                models::search::html_text(raw).to_lowercase()
            } else {
                raw.to_lowercase()
            }
        })
    }

    // Sender rows and actions need identities and flags, not copies of every body.
    fn header(&self) -> Message {
        Message {
            search_match: true,
            ..self.message.list_header(false)
        }
    }
}

struct Progress<'a> {
    request: &'a Request,
    query: Pairs,
    worker: &'a Shared,
    output: &'a async_channel::Sender<Response>,
    groups: Vec<(Option<Location>, Vec<Rc<Entry>>)>,
    location: Option<Location>,
    /// Positions in `groups` of the senders matched in `location`.
    senders: HashMap<String, usize>,
    last_sent: Option<Instant>,
}

impl Progress<'_> {
    /// Each location is visited once, so senders group within the current one.
    fn enter(&mut self, location: &Option<Location>) {
        self.location.clone_from(location);
        self.senders.clear();
    }

    fn visit(&mut self, entry: &Rc<Entry>) -> bool {
        if !current(self.worker, self.request) {
            return false;
        }
        if !entry.matches(self.request, &self.query) {
            return true;
        }
        let position = match self.senders.get(&entry.sender) {
            Some(&position) => position,
            None => {
                self.senders.insert(entry.sender.clone(), self.groups.len());
                self.groups.push((self.location.clone(), Vec::new()));
                self.groups.len() - 1
            }
        };
        self.groups[position].1.push(entry.clone());
        if self
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
            .map(|(location, group)| {
                group.sort_by_key(|entry| {
                    std::cmp::Reverse((entry.message.timestamp, entry.message.uid))
                });
                (&*location, &*group)
            })
            .collect();
        groups.sort_by_cached_key(|(location, group)| {
            std::cmp::Reverse((
                self.request.unread_first
                    && group.iter().any(|entry| {
                        !entry.message.is_read
                            || (location.as_ref().is_none_or(|location| {
                                self.request
                                    .scope
                                    .as_ref()
                                    .is_some_and(|scope| Some(location) == scope.current.as_ref())
                            }) && self.request.deferred_read.contains(&entry.message.uid))
                    }),
                group[0].message.timestamp,
                group[0].message.uid,
            ))
        });
        let (locations, groups) = groups
            .into_iter()
            .map(|(location, group)| {
                (
                    location.clone(),
                    group.iter().map(|entry| entry.header()).collect(),
                )
            })
            .unzip();
        self.last_sent = Some(Instant::now());
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
            query: Pairs::of([request.query.as_str()]),
            worker,
            output,
            groups: Vec::new(),
            location: None,
            senders: HashMap::new(),
            last_sent: None,
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
            progress.enter(&location);
            let mailbox = self.mailboxes.entry(location.clone()).or_default();
            let reusable = match &mailbox.source {
                Some(Source::Memory(source)) => memory && Arc::ptr_eq(source, &request.messages),
                Some(Source::Disk(old, rev)) => !memory && *old == version && *rev == revision,
                None => false,
            };
            if reusable {
                for entry in &mailbox.entries {
                    if !progress.visit(entry) {
                        return Ok(());
                    }
                }
                continue;
            }
            mailbox.source = None;
            let mut positions: HashMap<u32, usize> = mailbox
                .entries
                .iter()
                .enumerate()
                .map(|(position, entry)| (entry.message.uid, position))
                .collect();
            let mut seen = HashSet::new();
            let entries = &mut mailbox.entries;
            let mut visit = |message: Message| {
                if !current(worker, request) {
                    return false;
                }
                seen.insert(message.uid);
                let position = *positions.entry(message.uid).or_insert(entries.len());
                if position == entries.len() {
                    entries.push(Rc::new(Entry::new(message)));
                } else {
                    match Rc::get_mut(&mut entries[position]) {
                        Some(entry) if same_fields(&entry.message, &message) => {
                            entry.message = message;
                        }
                        _ => entries[position] = Rc::new(Entry::new(message)),
                    }
                }
                progress.visit(&entries[position])
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
            mailbox
                .entries
                .retain(|entry| seen.contains(&entry.message.uid));
            mailbox.entries.sort_by_key(|entry| {
                std::cmp::Reverse((entry.message.timestamp, entry.message.uid))
            });
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
        let matches = |request: &Request| entry.matches(request, &Pairs::of([&*request.query]));
        let mut request = Request::new(false, "delivery".into(), None, None, Arc::default());
        assert!(matches(&request));
        assert!(entry.fields[6].get().is_none());
        request.query = "visible & searchable".into();
        request.filters.sources[5] = false;
        assert!(!matches(&request));
        assert!(entry.fields[6].get().is_none());
        request.filters.sources[5] = true;
        assert!(matches(&request));
        request.query = "hidden".into();
        assert!(!matches(&request));
    }

    #[test]
    fn byte_pair_prefilter_never_drops_a_match() {
        let texts = [
            "Invoice 42 from ACME",
            "Ünïcödé Straße",
            "ΟΔΟΣ αγάπης",
            "🎉 party tonight",
            "meeting at noon",
            "zz top",
            "İstanbul",
            "x",
        ];
        let messages: Vec<_> = texts
            .iter()
            .enumerate()
            .flat_map(|(index, text)| {
                let uid = index as u32 * 3;
                let message = |uid, sender: String| Message {
                    uid,
                    timestamp: index as i64,
                    sender,
                    ..Default::default()
                };
                [
                    Message {
                        subject: (*text).into(),
                        ..message(uid + 1, format!("s{index}@example.com"))
                    },
                    Message {
                        body_text: format!("Body: {text}"),
                        ..message(uid + 2, format!("t{index}@example.com"))
                    },
                    Message {
                        cc: text.to_uppercase(),
                        ..message(uid + 3, format!("u{index}@example.com"))
                    },
                ]
            })
            .collect();
        let messages = Arc::new(messages);
        let (worker, results) = Search::start();
        for query in [
            "invoice", "42 f", "ünï", "straße", "οδος", "γάπ", "🎉", "party", "noon", "zz", "i̇st",
            "x", "acme", "body: z", "missing",
        ] {
            worker.submit(Request::new(
                false,
                query.into(),
                None,
                None,
                messages.clone(),
            ));
            let mut found: Vec<_> = super::super::tests::finished(&results)
                .groups
                .iter()
                .flatten()
                .map(|message| message.uid)
                .collect();
            found.sort();
            let expected: Vec<_> = messages
                .iter()
                .filter(|message| {
                    models::search::fields(message)
                        .iter()
                        .any(|field| field.to_lowercase().contains(query))
                })
                .map(|message| message.uid)
                .collect();
            assert_eq!(found, expected, "{query}");
        }
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
