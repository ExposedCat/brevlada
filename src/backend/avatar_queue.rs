use super::avatars::{self, Credential};
use crate::models::Account;
use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Condvar, Mutex},
};

#[derive(Default)]
struct Pending {
    closed: bool,
    requests: VecDeque<String>,
    queued: HashSet<String>,
    lookups: VecDeque<String>,
    lookup_queued: HashSet<String>,
    priority: HashSet<String>,
    focused: Option<String>,
    accounts: Vec<Account>,
    unauthorized: HashSet<String>,
}

/// Sender addresses waiting for an avatar lookup, plus the Google credentials
/// the lookups may use.
#[derive(Clone, Default)]
pub struct AvatarQueue(Arc<(Mutex<Pending>, Condvar)>);

impl AvatarQueue {
    pub fn retain_accounts(&self, accounts: &[Account]) {
        let mut pending = self.0.0.lock().unwrap();
        pending.accounts.retain(|existing| {
            accounts
                .iter()
                .any(|account| account.email == existing.email)
        });
        pending
            .unauthorized
            .retain(|email| accounts.iter().any(|account| &account.email == email));
    }
    pub fn register(&self, account: Account) {
        let mut pending = self.0.0.lock().unwrap();
        if let Some(existing) = pending
            .accounts
            .iter_mut()
            .find(|existing| existing.email == account.email)
        {
            *existing = account;
        } else {
            pending.accounts.push(account);
        }
    }

    /// The connected Google accounts, each marked with whether its contacts
    /// endpoints are still worth asking.
    pub fn credentials(&self) -> Vec<Credential> {
        let pending = self.0.0.lock().unwrap();
        pending
            .accounts
            .iter()
            .filter(|account| avatars::is_google(account))
            .map(|account| Credential {
                account: account.clone(),
                search: !pending.unauthorized.contains(&account.email),
            })
            .collect()
    }

    pub fn unauthorized(&self, emails: &[String]) {
        let mut pending = self.0.0.lock().unwrap();
        pending.unauthorized.extend(emails.iter().cloned());
    }

    pub fn push(&self, email: String) {
        let mut pending = self.0.0.lock().unwrap();
        if pending.closed || email.is_empty() || !pending.queued.insert(email.clone()) {
            return;
        }
        if pending.focused.as_deref() == Some(&email) {
            pending.priority.insert(email.clone());
            pending.requests.push_front(email);
        } else {
            pending.requests.push_back(email);
        }
        self.0.1.notify_all();
    }

    pub fn prioritize(&self, email: String) {
        let mut pending = self.0.0.lock().unwrap();
        if pending.closed || email.is_empty() {
            return;
        }
        pending.priority.insert(email.clone());
        pending.focused = Some(email.clone());
        if let Some(index) = pending.lookups.iter().position(|queued| queued == &email) {
            pending.lookups.remove(index);
            pending.lookups.push_front(email.clone());
        }
        if let Some(index) = pending.requests.iter().position(|queued| queued == &email) {
            pending.requests.remove(index);
            pending.requests.push_front(email);
        } else if !pending.lookup_queued.contains(&email) && pending.queued.insert(email.clone()) {
            pending.requests.push_front(email);
        }
        self.0.1.notify_all();
    }

    pub fn lookup(&self, email: String) {
        let mut pending = self.0.0.lock().unwrap();
        if pending.closed || !pending.lookup_queued.insert(email.clone()) {
            return;
        }
        if pending.priority.contains(&email) {
            pending.lookups.push_front(email);
        } else {
            pending.lookups.push_back(email);
        }
        self.0.1.notify_all();
    }

    pub fn pop_lookup(&self, priority_only: bool) -> Option<String> {
        let mut pending = self.0.0.lock().unwrap();
        loop {
            if pending.closed {
                return None;
            }
            let index = pending
                .lookups
                .iter()
                .position(|email| pending.priority.contains(email))
                .or_else(|| (!priority_only && !pending.lookups.is_empty()).then_some(0));
            if let Some(index) = index {
                return pending.lookups.remove(index);
            }
            pending = self.0.1.wait(pending).unwrap();
        }
    }

    pub fn finish_lookup(&self, email: &str) {
        let mut pending = self.0.0.lock().unwrap();
        pending.lookup_queued.remove(email);
        pending.priority.remove(email);
    }

    pub fn finish_cached(&self, email: &str) {
        self.0.0.lock().unwrap().priority.remove(email);
    }

    pub fn pop(&self) -> Option<String> {
        let mut pending = self.0.0.lock().unwrap();
        loop {
            if pending.closed {
                return None;
            }
            if let Some(email) = pending.requests.pop_front() {
                pending.queued.remove(&email);
                return Some(email);
            }
            pending = self.0.1.wait(pending).unwrap();
        }
    }

    pub fn close(&self) {
        let mut pending = self.0.0.lock().unwrap();
        pending.closed = true;
        pending.requests.clear();
        pending.lookups.clear();
        self.0.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(email: &str, host: &str) -> Account {
        Account {
            path: String::new(),
            email: email.into(),
            name: String::new(),
            host: host.into(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: true,
            smtp: None,
        }
    }

    #[test]
    fn queues_each_sender_once_until_it_is_taken() {
        let queue = AvatarQueue::default();
        queue.push("ada@example.com".into());
        queue.push("ada@example.com".into());
        queue.push("grace@example.com".into());
        queue.push(String::new());
        assert_eq!(queue.pop().unwrap(), "ada@example.com");
        assert_eq!(queue.pop().unwrap(), "grace@example.com");
        // Taking the request clears the guard, so a later refresh can requeue.
        queue.push("ada@example.com".into());
        assert_eq!(queue.pop().unwrap(), "ada@example.com");
        queue.close();
        queue.push("ada@example.com".into());
        assert!(queue.pop().is_none());
    }

    #[test]
    fn opening_a_sender_promotes_its_lookup_without_duplicating_active_work() {
        let queue = AvatarQueue::default();
        queue.push("background@example.com".into());
        queue.push("selected@example.com".into());
        queue.prioritize("selected@example.com".into());
        assert_eq!(queue.pop().unwrap(), "selected@example.com");
        assert_eq!(queue.pop().unwrap(), "background@example.com");
        queue.lookup("background@example.com".into());
        queue.lookup("selected@example.com".into());
        assert_eq!(queue.pop_lookup(true).unwrap(), "selected@example.com");
        queue.lookup("selected@example.com".into());
        queue.prioritize("selected@example.com".into());
        assert_eq!(queue.pop_lookup(false).unwrap(), "background@example.com");
        queue.finish_lookup("selected@example.com");
        queue.push("selected@example.com".into());
        assert_eq!(queue.pop().unwrap(), "selected@example.com");
        queue.lookup("selected@example.com".into());
        assert_eq!(queue.pop_lookup(true).unwrap(), "selected@example.com");
        queue.close();
        assert!(queue.pop_lookup(false).is_none());
        assert!(queue.pop().is_none());
    }

    #[test]
    fn reserved_worker_waits_for_an_opened_sender_and_wakes_on_shutdown() {
        use std::{sync::mpsc, time::Duration};
        let queue = AvatarQueue::default();
        queue.lookup("background@example.com".into());
        let (sent, received) = mpsc::channel();
        let pending = queue.clone();
        let worker = std::thread::spawn(move || {
            sent.send(pending.pop_lookup(true)).unwrap();
            sent.send(pending.pop_lookup(true)).unwrap();
        });
        assert!(received.recv_timeout(Duration::from_millis(50)).is_err());
        queue.prioritize("selected@example.com".into());
        assert_eq!(queue.pop().unwrap(), "selected@example.com");
        queue.lookup("selected@example.com".into());
        assert_eq!(
            received
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .as_deref(),
            Some("selected@example.com")
        );
        assert_eq!(queue.pop_lookup(false).unwrap(), "background@example.com");
        queue.close();
        assert!(
            received
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .is_none()
        );
        worker.join().unwrap();
    }

    #[test]
    fn offers_google_credentials_and_drops_contact_search_when_refused() {
        let queue = AvatarQueue::default();
        queue.register(account("me@gmail.com", "imap.gmail.com"));
        queue.register(account("work@example.com", "imap.fastmail.com"));
        assert_eq!(queue.credentials().len(), 1);
        assert!(queue.credentials()[0].search);
        // Re-registering the same account replaces it instead of duplicating.
        queue.register(account("me@gmail.com", "imap.gmail.com"));
        assert_eq!(queue.credentials().len(), 1);
        // A refused contacts lookup only drops the contacts endpoints. The
        // account stays so its owner's own photo is still resolved.
        queue.unauthorized(&["me@gmail.com".into()]);
        assert_eq!(queue.credentials().len(), 1);
        assert!(!queue.credentials()[0].search);
    }
}
