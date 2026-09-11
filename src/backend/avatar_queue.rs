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
    accounts: Vec<Account>,
    unauthorized: HashSet<String>,
}

/// Sender addresses waiting for an avatar lookup, plus the Google credentials
/// the lookups may use.
#[derive(Clone, Default)]
pub struct AvatarQueue(Arc<(Mutex<Pending>, Condvar)>);

impl AvatarQueue {
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
        pending.requests.push_back(email);
        self.0.1.notify_one();
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
