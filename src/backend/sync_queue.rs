use super::body_queue::BodyQueue;
use super::sync_progress::{Inventory, Tracker};
use crate::models::{
    Account,
    sync::{Activity, Status},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Condvar, Mutex},
    time::SystemTime,
};

pub const WORKERS: usize = 4;
const PER_ACCOUNT: usize = 2;

#[derive(Clone)]
pub enum Task {
    Discover,
    Scan,
    Headers {
        validity: u32,
        uids: Arc<Vec<u32>>,
        offset: usize,
    },
    Body {
        validity: u32,
        uid: u32,
        preview: bool,
        timestamp: i64,
    },
    Bodies {
        validity: u32,
        before: (i64, u32),
    },
}

#[derive(Clone)]
pub struct Job {
    pub account: Account,
    pub folder: String,
    pub task: Task,
}

type Key = (String, String, u8, u32, usize);

impl Job {
    fn key(&self) -> Key {
        let (kind, validity, item) = match &self.task {
            Task::Discover => (0, 0, 0),
            Task::Scan => (1, 0, 0),
            Task::Headers {
                validity, offset, ..
            } => (2, *validity, *offset),
            Task::Body { validity, uid, .. } => (3, *validity, *uid as usize),
            Task::Bodies { validity, before } => (4, *validity, before.1 as usize),
        };
        (
            self.account.email.clone(),
            self.folder.clone(),
            kind,
            validity,
            item,
        )
    }

    fn priority(&self, expanded: &HashSet<String>) -> (u8, u8, std::cmp::Reverse<i64>) {
        let (phase, timestamp) = match self.task {
            Task::Body {
                preview, timestamp, ..
            } => (if preview { 1 } else { 3 }, timestamp),
            Task::Bodies { before, .. } => (3, before.0),
            Task::Headers { offset, .. } if offset > 0 => (2, 0),
            _ => (0, 0),
        };
        let folder =
            if self.folder.eq_ignore_ascii_case("INBOX") || matches!(self.task, Task::Discover) {
                0
            } else if expanded.contains(&self.account.email) {
                1
            } else {
                2
            };
        (folder, phase, std::cmp::Reverse(timestamp))
    }
}

#[derive(Default)]
struct Pending {
    accounts: HashMap<String, Account>,
    expanded: HashSet<String>,
    folders: HashMap<String, Vec<String>>,
    jobs: Vec<Job>,
    known: HashSet<Key>,
    active: HashMap<String, usize>,
    closed: bool,
    visible: Option<(String, String)>,
    running: HashMap<Key, Job>,
    failed: usize,
    last_started: Option<SystemTime>,
    progress: Tracker,
}

impl Pending {
    fn next(&self) -> Option<usize> {
        let inbox_pending = self
            .jobs
            .iter()
            .chain(self.running.values())
            .any(|job| job.folder.eq_ignore_ascii_case("INBOX"));
        self.jobs
            .iter()
            .enumerate()
            .filter(|(_, job)| {
                self.active.get(&job.account.email).copied().unwrap_or(0) < PER_ACCOUNT
                    && (!inbox_pending
                        || job.folder.eq_ignore_ascii_case("INBOX")
                        || matches!(job.task, Task::Discover | Task::Scan))
            })
            .min_by_key(|(_, job)| job.priority(&self.expanded))
            .map(|(index, _)| index)
    }

    fn enqueue(&mut self, job: Job) {
        if self.closed {
            return;
        }
        if matches!(job.task, Task::Scan)
            && self.known.iter().any(|key| {
                key.0 == job.account.email && key.1 == job.folder && matches!(key.2, 1 | 2)
            })
        {
            return;
        }
        if self.known.is_empty() {
            self.failed = 0;
            self.progress = Tracker::default();
        }
        if self.known.insert(job.key()) {
            self.jobs.push(job);
        }
    }
}

#[derive(Clone, Default)]
pub struct SyncQueue {
    state: Arc<(Mutex<Pending>, Condvar)>,
    pub cancellation: BodyQueue,
}

impl SyncQueue {
    pub fn status(&self) -> Option<Status> {
        let state = self.state.0.try_lock().ok()?;
        let job = state
            .running
            .values()
            .min_by_key(|job| job.priority(&state.expanded))
            .or_else(|| state.jobs.first());
        Some(Status {
            activity: job.map(|job| Activity {
                description: match job.task {
                    Task::Discover => "Syncing folders",
                    Task::Scan | Task::Headers { .. } => "Syncing message lists",
                    Task::Body { preview: true, .. } => "Syncing previews",
                    Task::Body { .. } | Task::Bodies { .. } => "Syncing messages",
                },
                folder: job.folder.clone(),
                progress: state
                    .progress
                    .overall(state.accounts.keys().map(String::as_str)),
            }),
            active: state.running.len(),
            queued: state.jobs.len(),
            failed: state.failed,
            last_started: state.last_started,
        })
    }
    pub fn restore_started(&self, started: Option<SystemTime>) {
        self.state.0.lock().unwrap().last_started = started;
    }
    pub fn needs_start_time(&self) -> bool {
        let state = self.state.0.lock().unwrap();
        !state.closed && !state.accounts.is_empty() && state.known.is_empty()
    }
    pub fn restore_folders(&self, account: &str, folders: Vec<String>) {
        self.state
            .0
            .lock()
            .unwrap()
            .folders
            .insert(account.into(), folders);
    }
    pub fn discovered(&self, account: &str, folders: &[String]) -> Vec<String> {
        let mut state = self.state.0.lock().unwrap();
        let selected: Vec<_> = folders
            .iter()
            .filter(|folder| {
                state.folders.get(account).is_some_and(|selected| {
                    selected.iter().any(|item| {
                        item == *folder
                            || (item.eq_ignore_ascii_case("INBOX")
                                && folder.eq_ignore_ascii_case("INBOX"))
                    })
                })
            })
            .map(|folder| {
                if folder.eq_ignore_ascii_case("INBOX") {
                    "INBOX".into()
                } else {
                    folder.clone()
                }
            })
            .collect();
        state.progress.discovered(account, &selected);
        selected
    }
    pub fn inventory(&self, account: &str, folder: &str, inventory: Inventory) {
        self.state
            .0
            .lock()
            .unwrap()
            .progress
            .inventory(account, folder, inventory);
    }
    pub fn body_cached(&self, account: &str, folder: &str, validity: u32, uid: u32) {
        self.state
            .0
            .lock()
            .unwrap()
            .progress
            .body_cached(account, folder, validity, uid);
    }
    pub fn headers_cached(&self, account: &str, folder: &str, validity: u32) {
        self.state
            .0
            .lock()
            .unwrap()
            .progress
            .headers_cached(account, folder, validity);
    }
    pub fn focus(&self, account: &str, folder: &str) {
        self.state.0.lock().unwrap().visible = Some((account.into(), folder.into()));
    }

    pub fn visible(&self, account: &str, folder: &str) -> bool {
        self.state
            .0
            .lock()
            .unwrap()
            .visible
            .as_ref()
            .is_some_and(|(a, f)| a == account && f == folder)
    }

    #[cfg(test)]
    pub fn queued_len(&self) -> usize {
        self.state.0.lock().unwrap().jobs.len()
    }

    pub fn register(&self, account: Account, expanded: bool) {
        self.expanded(&account.email, expanded);
        self.state
            .0
            .lock()
            .unwrap()
            .accounts
            .insert(account.email.clone(), account);
    }

    pub fn expanded(&self, email: &str, expanded: bool) {
        let mut state = self.state.0.lock().unwrap();
        if expanded {
            state.expanded.insert(email.into());
        } else {
            state.expanded.remove(email);
        }
        self.state.1.notify_all();
    }

    pub fn refresh(&self) {
        let mut state = self.state.0.lock().unwrap();
        let accounts: Vec<_> = state.accounts.values().cloned().collect();
        for account in &accounts {
            if !state.folders.get(&account.email).is_some_and(|folders| {
                folders
                    .iter()
                    .any(|folder| folder.eq_ignore_ascii_case("INBOX"))
            }) {
                continue;
            }
            state.enqueue(Job {
                account: account.clone(),
                folder: "INBOX".into(),
                task: Task::Scan,
            });
        }
        for account in accounts {
            state.enqueue(Job {
                account,
                folder: String::new(),
                task: Task::Discover,
            });
        }
        self.state.1.notify_all();
    }

    pub fn push(&self, job: Job) {
        let mut state = self.state.0.lock().unwrap();
        state.enqueue(job);
        self.state.1.notify_all();
    }

    pub fn pop(&self) -> Option<Job> {
        let mut state = self.state.0.lock().unwrap();
        loop {
            if state.closed {
                return None;
            }
            let next = state.next();
            if let Some(index) = next {
                let job = state.jobs.remove(index);
                *state.active.entry(job.account.email.clone()).or_default() += 1;
                state.running.insert(job.key(), job.clone());
                return Some(job);
            }
            state = self.state.1.wait(state).unwrap();
        }
    }

    pub fn finish(&self, job: &Job, success: bool) {
        let mut state = self.state.0.lock().unwrap();
        state.known.remove(&job.key());
        state.running.remove(&job.key());
        state.failed += usize::from(!success);
        *state.active.get_mut(&job.account.email).unwrap() -= 1;
        self.state.1.notify_all();
    }

    pub fn close(&self) {
        let mut state = self.state.0.lock().unwrap();
        state.closed = true;
        state.jobs.clear();
        self.cancellation.close();
        self.state.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(email: &str, folder: &str, task: Task) -> Job {
        Job {
            account: Account {
                path: email.into(),
                email: email.into(),
                name: String::new(),
                host: String::new(),
                username: String::new(),
                port: 993,
                ssl: true,
                tls: false,
                oauth2: false,
            },
            folder: folder.into(),
            task,
        }
    }

    #[test]
    fn schedules_only_selected_folders_and_keeps_selection_when_registering() {
        let queue = SyncQueue::default();
        for (account, folders) in [
            ("default", vec!["INBOX".into()]),
            ("custom", vec!["Archive".into()]),
            ("empty", vec![]),
        ] {
            queue.restore_folders(account, folders);
            queue.register(job(account, "", Task::Discover).account, false);
        }
        queue.refresh();
        let available = vec!["INBOX".into(), "Archive".into(), "Sent".into()];
        let mut scanned = Vec::new();
        while queue.queued_len() > 0 {
            let next = queue.pop().unwrap();
            match next.task {
                Task::Discover => {
                    let selected = queue.discovered(&next.account.email, &available);
                    for folder in selected.into_iter().filter(|folder| folder != "INBOX") {
                        queue.push(job(&next.account.email, &folder, Task::Scan));
                    }
                }
                Task::Scan => scanned.push((next.account.email.clone(), next.folder.clone())),
                _ => panic!("Unexpected work"),
            }
            queue.finish(&next, true);
        }
        scanned.sort();
        assert_eq!(
            scanned,
            vec![
                ("custom".into(), "Archive".into()),
                ("default".into(), "INBOX".into())
            ]
        );
        queue.close();
    }

    #[test]
    fn status_preserves_sync_start_time_after_completion_failure_or_cancellation() {
        let queue = SyncQueue::default();
        assert!(queue.status().unwrap().last_started.is_none());
        let started = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        queue.restore_started(Some(started));
        queue.push(job("a", "INBOX", Task::Scan));
        queue.push(job("b", "INBOX", Task::Scan));
        assert_eq!(queue.status().unwrap().queued, 2);
        let first = queue.pop().unwrap();
        let second = queue.pop().unwrap();
        assert_eq!(queue.status().unwrap().active, 2);
        queue.finish(&first, true);
        assert!(queue.status().unwrap().activity.is_some());
        assert_eq!(queue.status().unwrap().last_started, Some(started));
        queue.finish(&second, true);
        let completed = queue.status().unwrap().last_started.unwrap();
        assert!(queue.status().unwrap().activity.is_none());
        queue.push(job("a", "INBOX", Task::Scan));
        let failed = queue.pop().unwrap();
        queue.finish(&failed, false);
        assert_eq!(queue.status().unwrap().failed, 1);
        assert_eq!(queue.status().unwrap().last_started, Some(completed));
        queue.push(job("a", "INBOX", Task::Scan));
        assert_eq!(queue.status().unwrap().failed, 0);
        let cancelled = queue.pop().unwrap();
        queue.close();
        queue.finish(&cancelled, true);
        assert_eq!(queue.status().unwrap().last_started, Some(completed));
    }

    #[test]
    fn prioritizes_inboxes_then_expanded_accounts_and_orders_work_within_each_tier() {
        let queue = SyncQueue::default();
        queue.push(job("a", "Archive", Task::Scan));
        queue.push(job("b", "Work", Task::Scan));
        queue.push(job(
            "a",
            "INBOX",
            Task::Body {
                validity: 1,
                uid: 1,
                preview: false,
                timestamp: 1,
            },
        ));
        queue.push(job(
            "a",
            "INBOX",
            Task::Body {
                validity: 1,
                uid: 2,
                preview: true,
                timestamp: 2,
            },
        ));
        queue.push(job("c", "INBOX", Task::Scan));
        queue.expanded("b", true);
        let expected = [
            ("c", "INBOX"),
            ("a", "INBOX"),
            ("a", "INBOX"),
            ("b", "Work"),
            ("a", "Archive"),
        ];
        for (index, (email, folder)) in expected.into_iter().enumerate() {
            let job = queue.pop().unwrap();
            assert_eq!(
                (job.account.email.as_str(), job.folder.as_str()),
                (email, folder)
            );
            if index == 1 {
                assert!(matches!(job.task, Task::Body { preview: true, .. }));
            }
            queue.finish(&job, true);
        }
        queue.close();
        assert!(queue.pop().is_none());
    }

    #[test]
    fn other_folders_wait_for_running_inboxes_across_accounts() {
        let queue = SyncQueue::default();
        queue.push(job(
            "a",
            "INBOX",
            Task::Body {
                validity: 1,
                uid: 1,
                preview: false,
                timestamp: 1,
            },
        ));
        queue.push(job(
            "b",
            "Archive",
            Task::Body {
                validity: 1,
                uid: 2,
                preview: true,
                timestamp: 100,
            },
        ));
        let inbox_a = queue.pop().unwrap();
        assert_eq!(inbox_a.folder, "INBOX");
        assert!(queue.state.0.lock().unwrap().next().is_none());
        queue.push(job("b", "Work", Task::Scan));
        let inventory = queue.pop().unwrap();
        assert!(matches!(inventory.task, Task::Scan));
        queue.finish(&inventory, true);
        queue.push(job("b", "INBOX", Task::Scan));
        let inbox_b = queue.pop().unwrap();
        assert_eq!(inbox_b.folder, "INBOX");
        queue.push(job(
            "b",
            "INBOX",
            Task::Body {
                validity: 1,
                uid: 3,
                preview: true,
                timestamp: 2,
            },
        ));
        queue.finish(&inbox_b, true);
        let body_b = queue.pop().unwrap();
        assert_eq!(body_b.folder, "INBOX");
        queue.finish(&inbox_a, true);
        assert!(queue.state.0.lock().unwrap().next().is_none());
        queue.finish(&body_b, true);
        let archive = queue.pop().unwrap();
        assert_eq!(archive.folder, "Archive");
        queue.finish(&archive, true);
        queue.close();
    }

    #[test]
    fn deduplicates_active_work_and_caps_parallel_requests_per_account() {
        let queue = SyncQueue::default();
        let first = job("a", "INBOX", Task::Scan);
        queue.push(first.clone());
        queue.push(first.clone());
        let active = queue.pop().unwrap();
        queue.push(first);
        queue.push(job("a", "Work", Task::Scan));
        queue.push(job("a", "Archive", Task::Scan));
        let second = queue.pop().unwrap();
        queue.push(job("b", "Archive", Task::Scan));
        let third = queue.pop().unwrap();
        assert_eq!(third.account.email, "b");
        queue.finish(&active, true);
        assert_eq!(queue.pop().unwrap().folder, "Archive");
        queue.finish(&second, true);
        queue.finish(&third, true);
        queue.close();
    }
}
