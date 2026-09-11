use crate::models::sync::{Count, Progress};
use std::collections::{HashMap, HashSet};

pub struct Inventory {
    pub validity: u32,
    pub live: Vec<u32>,
    pub needed: Vec<u32>,
    pub headers_cached: bool,
}

#[derive(Default)]
pub struct Tracker(HashMap<String, Account>);

#[derive(Default)]
struct Account {
    folders: Option<HashSet<String>>,
    inventories: HashMap<String, Folder>,
}

struct Folder {
    validity: u32,
    needed: HashSet<u32>,
    cached: HashSet<u32>,
    headers_cached: bool,
}

impl Tracker {
    pub fn overall<'a>(&self, accounts: impl Iterator<Item = &'a str>) -> Progress {
        let mut account_count = Count {
            cached: 0,
            total: 0,
        };
        let mut folders = Some(Count {
            cached: 0,
            total: 0,
        });
        let mut messages = Some(Count {
            cached: 0,
            total: 0,
        });
        for account in accounts {
            account_count.total += 1;
            let progress = self.progress(account);
            account_count.cached += usize::from(
                progress
                    .folders
                    .as_ref()
                    .is_some_and(|count| count.cached == count.total),
            );
            for (sum, count) in [
                (&mut folders, progress.folders),
                (&mut messages, progress.messages),
            ] {
                if let (Some(sum), Some(count)) = (sum.as_mut(), count.as_ref()) {
                    sum.cached += count.cached;
                    sum.total += count.total;
                } else {
                    *sum = None;
                }
            }
        }
        if account_count.total == 0 {
            return Progress::default();
        }
        Progress {
            accounts: Some(account_count),
            folders,
            messages,
        }
    }

    pub fn discovered(&mut self, account: &str, folders: &[String]) {
        self.0.entry(account.into()).or_default().folders =
            Some(folders.iter().map(|folder| key(folder)).collect());
    }

    pub fn inventory(&mut self, account: &str, folder: &str, inventory: Inventory) {
        let account = self.0.entry(account.into()).or_default();
        let folder = account
            .inventories
            .entry(key(folder))
            .or_insert_with(|| Folder {
                validity: inventory.validity,
                needed: HashSet::new(),
                cached: HashSet::new(),
                headers_cached: false,
            });
        if folder.validity != inventory.validity {
            folder.needed.clear();
            folder.cached.clear();
            folder.validity = inventory.validity;
        }
        let live: HashSet<_> = inventory.live.into_iter().collect();
        let needed: HashSet<_> = inventory.needed.into_iter().collect();
        folder.needed.retain(|uid| live.contains(uid));
        folder.cached.retain(|uid| live.contains(uid));
        folder
            .cached
            .extend(folder.needed.difference(&needed).copied());
        folder.needed.extend(needed);
        folder.headers_cached = inventory.headers_cached;
    }

    pub fn body_cached(&mut self, account: &str, folder: &str, validity: u32, uid: u32) {
        if let Some(folder) = self
            .0
            .get_mut(account)
            .and_then(|a| a.inventories.get_mut(&key(folder)))
            && folder.validity == validity
            && folder.needed.contains(&uid)
        {
            folder.cached.insert(uid);
        }
    }

    pub fn headers_cached(&mut self, account: &str, folder: &str, validity: u32) {
        if let Some(folder) = self
            .0
            .get_mut(account)
            .and_then(|a| a.inventories.get_mut(&key(folder)))
            && folder.validity == validity
        {
            folder.headers_cached = true;
        }
    }

    pub fn progress(&self, account: &str) -> Progress {
        let Some(account) = self.0.get(account) else {
            return Progress::default();
        };
        let Some(folders) = &account.folders else {
            return Progress::default();
        };
        let mut complete = 0;
        let mut cached = 0;
        let mut total = 0;
        let mut known = true;
        for name in folders {
            if let Some(folder) = account.inventories.get(name) {
                let count = folder.cached.len();
                cached += count;
                total += folder.needed.len();
                complete += usize::from(folder.headers_cached && count == folder.needed.len());
            } else {
                known = false;
            }
        }
        Progress {
            accounts: None,
            folders: Some(Count {
                cached: complete,
                total: folders.len(),
            }),
            messages: known.then_some(Count { cached, total }),
        }
    }
}

fn key(folder: &str) -> String {
    if folder.eq_ignore_ascii_case("INBOX") {
        "INBOX".into()
    } else {
        folder.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_accounts_and_hides_totals_until_every_account_is_known() {
        let mut tracker = Tracker::default();
        tracker.discovered("a", &["INBOX".into()]);
        tracker.inventory(
            "a",
            "INBOX",
            Inventory {
                validity: 1,
                live: vec![],
                needed: vec![],
                headers_cached: true,
            },
        );
        let progress = tracker.overall(["a", "b"].into_iter());
        assert_eq!(
            progress.accounts,
            Some(Count {
                cached: 1,
                total: 2
            })
        );
        assert!(progress.folders.is_none() && progress.messages.is_none());
        tracker.discovered("b", &["Work".into()]);
        tracker.inventory(
            "b",
            "Work",
            Inventory {
                validity: 2,
                live: vec![1, 2],
                needed: vec![1, 2],
                headers_cached: true,
            },
        );
        let progress = tracker.overall(["a", "b"].into_iter());
        assert_eq!(
            progress.folders,
            Some(Count {
                cached: 1,
                total: 2
            })
        );
        assert_eq!(
            progress.messages,
            Some(Count {
                cached: 0,
                total: 2
            })
        );
        tracker.body_cached("b", "Work", 2, 1);
        tracker.body_cached("b", "Work", 2, 2);
        assert_eq!(
            tracker.overall(["a", "b"].into_iter()),
            Progress {
                accounts: Some(Count {
                    cached: 2,
                    total: 2
                }),
                folders: Some(Count {
                    cached: 2,
                    total: 2
                }),
                messages: Some(Count {
                    cached: 2,
                    total: 2
                }),
            }
        );
    }

    #[test]
    fn hides_unknown_totals_and_counts_only_cached_content_per_account() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.progress("a"), Progress::default());
        tracker.inventory(
            "a",
            "INBOX",
            Inventory {
                validity: 1,
                live: vec![1, 2],
                needed: vec![1, 2],
                headers_cached: true,
            },
        );
        tracker.discovered("a", &["Inbox".into(), "Work".into()]);
        assert_eq!(
            tracker.progress("a").folders,
            Some(Count {
                cached: 0,
                total: 2
            })
        );
        assert!(tracker.progress("a").messages.is_none());
        tracker.inventory(
            "a",
            "Work",
            Inventory {
                validity: 2,
                live: vec![1],
                needed: vec![1],
                headers_cached: false,
            },
        );
        assert_eq!(
            tracker.progress("a").messages,
            Some(Count {
                cached: 0,
                total: 3
            })
        );
        tracker.body_cached("a", "INBOX", 1, 1);
        tracker.body_cached("a", "INBOX", 1, 1);
        tracker.body_cached("a", "Work", 1, 1);
        assert_eq!(
            tracker.progress("a").messages,
            Some(Count {
                cached: 1,
                total: 3
            })
        );
        tracker.body_cached("a", "INBOX", 1, 2);
        tracker.body_cached("a", "Work", 2, 1);
        assert_eq!(
            tracker.progress("a").folders,
            Some(Count {
                cached: 1,
                total: 2
            })
        );
        tracker.headers_cached("a", "Work", 2);
        assert_eq!(
            tracker.progress("a").folders,
            Some(Count {
                cached: 2,
                total: 2
            })
        );
        assert_eq!(
            tracker.progress("a").messages,
            Some(Count {
                cached: 3,
                total: 3
            })
        );
        assert_eq!(tracker.progress("b"), Progress::default());
        tracker.inventory(
            "a",
            "INBOX",
            Inventory {
                validity: 1,
                live: vec![1, 2, 3],
                needed: vec![3],
                headers_cached: false,
            },
        );
        assert_eq!(
            tracker.progress("a").messages,
            Some(Count {
                cached: 3,
                total: 4
            })
        );
        tracker.inventory(
            "a",
            "INBOX",
            Inventory {
                validity: 9,
                live: vec![1],
                needed: vec![1],
                headers_cached: false,
            },
        );
        assert_eq!(
            tracker.progress("a").messages,
            Some(Count {
                cached: 1,
                total: 2
            })
        );
    }
}
