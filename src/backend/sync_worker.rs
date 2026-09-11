use super::{
    connections::Connections,
    storage::Storage,
    sync_progress::Inventory,
    sync_queue::{Job, SyncQueue, Task, WORKERS},
    worker::Event,
};
use crate::theme;
use anyhow::Result;
use std::{path::PathBuf, sync::Arc};

const HEADER_BATCH: usize = 50;
const BODY_BATCH: usize = 128;

pub fn start(path: PathBuf, queue: SyncQueue, events: async_channel::Sender<Event>) {
    for _ in 0..WORKERS {
        let (path, queue, events) = (path.clone(), queue.clone(), events.clone());
        std::thread::spawn(move || {
            let mut connections = Connections::default();
            let mut storage = match Storage::open(&path) {
                Ok(storage) => storage,
                Err(error) => {
                    eprintln!("Could not open background mail cache: {error}");
                    return;
                }
            };
            while let Some(job) = queue.pop() {
                let result = execute(&mut storage, &queue, &events, &mut connections, &job);
                if let Err(error) = &result
                    && queue.cancellation.current(0)
                {
                    eprintln!(
                        "Background mail sync failed for {} / {}: {error}",
                        job.account.email, job.folder
                    );
                }
                queue.finish(&job, result.is_ok());
            }
        });
    }
}

fn execute(
    storage: &mut Storage,
    queue: &SyncQueue,
    events: &async_channel::Sender<Event>,
    connections: &mut Connections,
    job: &Job,
) -> Result<()> {
    let (account, folder) = (&job.account, &job.folder);
    match &job.task {
        Task::Discover => {
            let (all, folders) =
                connections
                    .execute_body(account, &queue.cancellation, 0, |mail| mail.sync_folders())?;
            storage.store_folders(&account.email, &all)?;
            let folders = queue.discovered(&account.email, &folders);
            events.send_blocking(Event::Folders(account.email.clone(), all))?;
            for folder in folders
                .into_iter()
                .filter(|folder| !folder.eq_ignore_ascii_case("INBOX"))
            {
                queue.push(Job {
                    account: account.clone(),
                    folder,
                    task: Task::Scan,
                });
            }
        }
        Task::Scan => {
            let (validity, flags) =
                connections.execute_body(account, &queue.cancellation, 0, |mail| {
                    mail.inventory(folder)
                })?;
            let missing = storage.inventory(&account.email, folder, validity, &flags)?;
            let mut needed = storage.uncached_uids(&account.email, folder)?;
            needed.extend(missing.iter().copied());
            queue.inventory(
                &account.email,
                folder,
                Inventory {
                    validity,
                    live: flags.iter().map(|flag| flag.uid).collect(),
                    needed,
                    headers_cached: missing.is_empty(),
                },
            );
            events.send_blocking(Event::Unread(
                account.email.clone(),
                vec![(folder.clone(), flags.iter().any(|item| !item.read))],
            ))?;
            publish_list(storage, queue, events, job)?;
            if missing.is_empty() {
                enqueue_bodies(storage, queue, job, validity, None)?;
            } else {
                queue.push(Job {
                    task: Task::Headers {
                        validity,
                        uids: Arc::new(missing),
                        offset: 0,
                    },
                    ..job.clone()
                });
            }
        }
        Task::Headers {
            validity,
            uids,
            offset,
        } => {
            let end = (*offset + HEADER_BATCH).min(uids.len());
            let messages = connections.execute_body(account, &queue.cancellation, 0, |mail| {
                mail.header_batch(folder, *validity, &uids[*offset..end])
            })?;
            storage.store_headers(&account.email, folder, *validity, &messages)?;
            if end == uids.len() {
                queue.headers_cached(&account.email, folder, *validity);
            }
            if *offset == 0 || end == uids.len() {
                publish_list(storage, queue, events, job)?;
                enqueue_bodies(storage, queue, job, *validity, None)?;
            }
            if end < uids.len() {
                queue.push(Job {
                    task: Task::Headers {
                        validity: *validity,
                        uids: uids.clone(),
                        offset: end,
                    },
                    ..job.clone()
                });
            }
        }
        Task::Bodies { validity, before } => {
            if storage.validity(&account.email, folder)? == Some(*validity) {
                enqueue_bodies(storage, queue, job, *validity, Some(*before))?;
            }
        }
        Task::Body { validity, uid, .. } => {
            if storage.validity(&account.email, folder)? != Some(*validity) {
                return Ok(());
            }
            let Some(message) = storage.message(&account.email, folder, *uid)? else {
                return Ok(());
            };
            if message.body_loaded {
                queue.body_cached(&account.email, folder, *validity, *uid);
                return Ok(());
            }
            let fetched = connections.execute_body(account, &queue.cancellation, 0, |mail| {
                mail.cached_body(folder, *validity, *uid)
            })?;
            if let Some(message) =
                storage.store_body(&account.email, folder, Some(*validity), &fetched)?
            {
                queue.body_cached(&account.email, folder, *validity, *uid);
                if queue.visible(&account.email, folder) {
                    events.send_blocking(Event::CacheBody(
                        account.email.clone(),
                        folder.clone(),
                        message,
                    ))?;
                }
            }
        }
    }
    Ok(())
}

fn publish_list(
    storage: &Storage,
    queue: &SyncQueue,
    events: &async_channel::Sender<Event>,
    job: &Job,
) -> Result<()> {
    if !queue.visible(&job.account.email, &job.folder) {
        return Ok(());
    }
    events.send_blocking(Event::CacheList(
        job.account.email.clone(),
        job.folder.clone(),
        storage.messages(&job.account.email, &job.folder)?,
    ))?;
    Ok(())
}

fn enqueue_bodies(
    storage: &Storage,
    queue: &SyncQueue,
    job: &Job,
    validity: u32,
    before: Option<(i64, u32)>,
) -> Result<()> {
    let messages = storage.pending_bodies(&job.account.email, &job.folder, before, BODY_BATCH)?;
    for (index, message) in messages.iter().enumerate() {
        queue.push(Job {
            task: Task::Body {
                validity,
                uid: message.uid,
                preview: before.is_none() && index < theme::MESSAGE_LIMIT,
                timestamp: message.timestamp,
            },
            ..job.clone()
        });
    }
    if messages.len() == BODY_BATCH
        && let Some(last) = messages.last()
    {
        queue.push(Job {
            task: Task::Bodies {
                validity,
                before: (last.timestamp, last.uid),
            },
            ..job.clone()
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Account, Message};

    #[test]
    fn large_mailboxes_fill_a_bounded_queue_and_resume_after_restart() {
        let directory =
            std::env::temp_dir().join(format!("brevlada-sync-{}", gtk::glib::uuid_string_random()));
        let path = directory.join("cache.db");
        let mut storage = Storage::open(&path).unwrap();
        let job = Job {
            account: Account {
                path: "a".into(),
                email: "a".into(),
                name: String::new(),
                host: String::new(),
                username: String::new(),
                port: 993,
                ssl: true,
                tls: false,
                oauth2: false,
            },
            folder: "INBOX".into(),
            task: Task::Scan,
        };
        let flags: Vec<_> = (1..=300)
            .map(|uid| super::super::mail_sync::Flags {
                uid,
                read: false,
                flagged: false,
            })
            .collect();
        storage.inventory("a", "INBOX", 1, &flags).unwrap();
        let messages: Vec<_> = (1..=300)
            .map(|uid| Message {
                uid,
                timestamp: uid as i64,
                ..Default::default()
            })
            .collect();
        storage.store_headers("a", "INBOX", 1, &messages).unwrap();
        let queue = SyncQueue::default();
        enqueue_bodies(&storage, &queue, &job, 1, None).unwrap();
        assert_eq!(queue.queued_len(), BODY_BATCH + 1);
        let first = queue.pop().unwrap();
        assert!(matches!(
            first.task,
            Task::Body {
                uid: 300,
                preview: true,
                ..
            }
        ));
        storage
            .store_body(
                "a",
                "INBOX",
                Some(1),
                &Message {
                    body_loaded: true,
                    body_text: "Persisted".into(),
                    ..messages[299].clone()
                },
            )
            .unwrap();
        queue.finish(&first, true);
        queue.close();
        drop(storage);
        let mut storage = Storage::open(&path).unwrap();
        assert!(
            storage
                .inventory("a", "INBOX", 1, &flags)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            storage
                .message("a", "INBOX", 300)
                .unwrap()
                .unwrap()
                .body_text,
            "Persisted"
        );
        let queue = SyncQueue::default();
        enqueue_bodies(&storage, &queue, &job, 1, None).unwrap();
        let mut seen = std::collections::HashSet::new();
        while queue.queued_len() > 0 {
            assert!(queue.queued_len() <= BODY_BATCH + 1);
            let next = queue.pop().unwrap();
            match next.task {
                Task::Body { uid, .. } => {
                    assert!(seen.insert(uid));
                }
                Task::Bodies { validity, before } => {
                    enqueue_bodies(&storage, &queue, &next, validity, Some(before)).unwrap()
                }
                _ => panic!("Expected cached-body work"),
            }
            queue.finish(&next, true);
        }
        assert_eq!(seen.len(), 299);
        assert!(!seen.contains(&300));
        queue.close();
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
