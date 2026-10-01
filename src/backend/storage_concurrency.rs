use super::*;
use crate::backend::{avatars::Source, mail_sync::Flags};
use std::{
    path::PathBuf,
    sync::{Barrier, mpsc},
    time::{Duration, SystemTime},
};

struct Cache(PathBuf);

impl Cache {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "brevlada-concurrency-{}",
            gtk::glib::uuid_string_random()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> PathBuf {
        self.0.join("cache.db")
    }
}

impl Drop for Cache {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn opening_an_active_cache_does_not_request_a_sqlite_write_lock() {
    let cache = Cache::new();
    let storage = Storage::open(&cache.path()).unwrap();
    let mut external = Connection::open(cache.path()).unwrap();
    let transaction = external
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    // Another process may hold a SQLite write lock. Opening an initialized cache
    // should still succeed, rather than rerun migrations and time out.
    let reopened = Storage::open(&cache.path()).unwrap();
    assert!(Arc::ptr_eq(&storage.1, &reopened.1));
    assert!(reopened.folders("a").unwrap().is_empty());
    let alias = Storage::open(&cache.0.join(".").join("cache.db")).unwrap();
    assert!(Arc::ptr_eq(&storage.1, &alias.1));
    drop(transaction);
    reopened.store_unread("a", "INBOX", true).unwrap();
    assert_eq!(storage.unread("a").unwrap(), [("INBOX".into(), true)]);
}

#[test]
fn competing_writes_wait_while_reads_continue() {
    let cache = Cache::new();
    let mut first = Storage::open(&cache.path()).unwrap();
    let second = Storage::open(&cache.path()).unwrap();
    let reader = Storage::open_read_only(&cache.path()).unwrap();
    first.store_unread("a", "INBOX", false).unwrap();
    // Without application coordination, SQLite would immediately fail the
    // second write with DatabaseBusy while the first transaction is active.
    second.0.busy_timeout(Duration::ZERO).unwrap();
    let write = first.1.lock().unwrap();
    let transaction = first
        .0
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    transaction
        .execute("UPDATE rust_unread SET unread=1", [])
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx
            .send(second.store_unread("a", "Archive", true))
            .unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let waiting = done_rx.recv_timeout(Duration::from_millis(100));
    let during_write = reader.unread("a").unwrap();
    transaction.commit().unwrap();
    drop(write);
    thread.join().unwrap();
    assert!(matches!(waiting, Err(mpsc::RecvTimeoutError::Timeout)));
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert_eq!(during_write, [("INBOX".into(), false)]);
    assert_eq!(
        reader.unread("a").unwrap(),
        [("Archive".into(), true), ("INBOX".into(), true)]
    );
}

#[test]
fn concurrent_startup_and_mixed_cache_writes_succeed_without_a_busy_timeout() {
    let cache = Cache::new();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|worker| {
            let path = cache.path();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut storage = Storage::open(&path).unwrap();
                storage.0.busy_timeout(Duration::ZERO).unwrap();
                barrier.wait();
                let account = format!("account-{worker}");
                storage.store_folders(&account, &["INBOX".into()]).unwrap();
                storage.sync_folders(&account).unwrap();
                for uid in 1..=20 {
                    let message = Message {
                        uid,
                        uid_validity: Some(7),
                        message_id: format!("{account}-{uid}"),
                        body_loaded: true,
                        body_text: "Cached body".into(),
                        ..Default::default()
                    };
                    storage
                        .inventory(
                            &account,
                            "INBOX",
                            7,
                            &(1..=uid)
                                .map(|uid| Flags {
                                    uid,
                                    read: false,
                                    flagged: false,
                                    draft: false,
                                })
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                    storage
                        .store_headers(&account, "INBOX", 7, std::slice::from_ref(&message))
                        .unwrap();
                    storage
                        .store_body(&account, "INBOX", Some(7), &message)
                        .unwrap();
                    storage
                        .mark_read_cached(&account, "INBOX", Some(7), uid)
                        .unwrap();
                    storage.store_sent(&account, &message).unwrap();
                    storage
                        .store_avatar(&message.message_id, Source::None, None)
                        .unwrap();
                    storage.store_unread(&account, "INBOX", false).unwrap();
                    storage.record_sync_started(SystemTime::now()).unwrap();
                }
                assert_eq!(storage.messages(&account, "INBOX").unwrap().len(), 20);
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn failed_transactions_release_the_write_gate_and_memory_caches_are_independent() {
    let cache = Cache::new();
    let mut first = Storage::open(&cache.path()).unwrap();
    let second = Storage::open(&cache.path()).unwrap();
    let message = Message {
        uid: 1,
        uid_validity: Some(7),
        ..Default::default()
    };
    // There is no folder with this UIDVALIDITY. The transaction must roll back
    // and release the gate before another connection saves anything.
    assert!(first.store_headers("a", "INBOX", 7, &[message]).is_err());
    second.store_unread("a", "INBOX", true).unwrap();
    assert_eq!(first.read_revision().unwrap(), 0);
    let memory = Storage::open(Path::new(":memory:")).unwrap();
    let other_memory = Storage::open(Path::new(":memory:")).unwrap();
    assert!(!Arc::ptr_eq(&memory.1, &other_memory.1));
    memory.store_unread("a", "INBOX", true).unwrap();
    assert!(other_memory.unread("a").unwrap().is_empty());
}
