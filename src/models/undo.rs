use super::{Account, Message, action_target::ActionTarget, sender_action::SenderAction};
use std::collections::VecDeque;

#[derive(Clone)]
pub struct Snapshot {
    pub target: ActionTarget,
    pub opened: Vec<(u32, String)>,
    pub account: String,
    pub folder: String,
    pub messages: Vec<Message>,
}

#[derive(Clone)]
pub struct Batch {
    pub account: Account,
    pub folder: String,
    pub destination: String,
    pub messages: Vec<Message>,
}

#[derive(Clone)]
pub struct Action {
    pub id: u64,
    pub action: SenderAction,
    pub batches: Vec<Batch>,
}

struct Entry {
    action: Action,
    pending: Vec<(String, String, ActionTarget)>,
    requested: bool,
    snapshots: Vec<Snapshot>,
}

#[derive(Default)]
pub struct History {
    next: u64,
    entries: VecDeque<Entry>,
}

impl History {
    pub fn begin(&mut self, action: SenderAction) -> u64 {
        self.next += 1;
        self.entries.push_back(Entry {
            action: Action {
                id: self.next,
                action,
                batches: Vec::new(),
            },
            pending: Vec::new(),
            requested: false,
            snapshots: Vec::new(),
        });
        self.next
    }

    pub fn register(&mut self, id: u64, account: &str, folder: &str, target: &ActionTarget) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.action.id == id) {
            entry
                .pending
                .push((account.into(), folder.into(), target.clone()));
        }
        self.trim();
    }

    pub fn remember(&mut self, id: u64, snapshot: Snapshot) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.action.id == id) {
            entry.snapshots.push(snapshot);
        }
    }

    pub fn snapshots(&self, id: u64) -> Vec<Snapshot> {
        let Some(entry) = self.entries.iter().find(|entry| entry.action.id == id) else {
            return Vec::new();
        };
        if entry.snapshots.is_empty() {
            return entry
                .action
                .batches
                .iter()
                .map(|batch| Snapshot {
                    target: ActionTarget::Messages(
                        batch
                            .messages
                            .iter()
                            .map(|message| (message.uid, message.message_id.clone()))
                            .collect(),
                    ),
                    opened: Vec::new(),
                    account: batch.account.email.clone(),
                    folder: batch.folder.clone(),
                    messages: batch.messages.clone(),
                })
                .collect();
        }
        entry
            .snapshots
            .iter()
            .filter_map(|snapshot| {
                let pending = entry.pending.contains(&(
                    snapshot.account.clone(),
                    snapshot.folder.clone(),
                    snapshot.target.clone(),
                ));
                let mut messages = if pending {
                    snapshot.messages.clone()
                } else {
                    Vec::new()
                };
                for batch in &entry.action.batches {
                    if batch.account.email == snapshot.account && batch.folder == snapshot.folder {
                        for message in batch
                            .messages
                            .iter()
                            .filter(|message| snapshot.target.matches(message))
                        {
                            if !messages.iter().any(|existing| {
                                existing.uid == message.uid
                                    && existing.message_id == message.message_id
                            }) {
                                messages.push(message.clone());
                            }
                        }
                    }
                }
                (!messages.is_empty()).then(|| Snapshot {
                    opened: snapshot
                        .opened
                        .iter()
                        .filter(|(uid, id)| {
                            messages
                                .iter()
                                .any(|message| message.uid == *uid && message.message_id == *id)
                        })
                        .cloned()
                        .collect(),
                    target: snapshot.target.clone(),
                    account: snapshot.account.clone(),
                    folder: snapshot.folder.clone(),
                    messages,
                })
            })
            .collect()
    }

    pub fn cancel_pending(&mut self, id: u64, account: &str, folder: &str, target: &ActionTarget) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.action.id == id) {
            entry
                .pending
                .retain(|(a, f, t)| a != account || f != folder || t != target);
        }
        self.entries
            .retain(|entry| !entry.pending.is_empty() || !entry.action.batches.is_empty());
    }

    pub fn retarget(
        &mut self,
        id: u64,
        mailbox: (&str, &str),
        old: &ActionTarget,
        new: &ActionTarget,
        before: &[Message],
        after: &[Message],
    ) {
        let (account, folder) = mailbox;
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.action.id == id) {
            for (a, f, target) in &mut entry.pending {
                if a == account && f == folder && target == old {
                    *target = new.clone();
                }
            }
            for snapshot in &mut entry.snapshots {
                if snapshot.account == account && snapshot.folder == folder {
                    if &snapshot.target == old {
                        snapshot.target = new.clone();
                    }
                    for (uid, message_id) in &mut snapshot.opened {
                        if let Some(index) = before.iter().position(|message| {
                            message.uid == *uid && message.message_id == *message_id
                        }) && let Some(message) = after.get(index)
                        {
                            *uid = message.uid;
                        }
                    }
                    for message in &mut snapshot.messages {
                        if let Some(index) = before.iter().position(|old| {
                            old.uid == message.uid && old.message_id == message.message_id
                        }) && let Some(replacement) = after.get(index)
                        {
                            *message = replacement.clone();
                        }
                    }
                }
            }
        }
    }

    pub fn complete(
        &mut self,
        account: &Account,
        folder: &str,
        target: &ActionTarget,
        destination: Option<String>,
        mut messages: Vec<Message>,
    ) -> Vec<Snapshot> {
        let key = (account.email.clone(), folder.into(), target.clone());
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.pending.contains(&key))
        {
            entry.pending.retain(|pending| pending != &key);
            if let Some(destination) = destination
                && !messages.is_empty()
            {
                for message in &mut messages {
                    if let Some(cached) = entry
                        .snapshots
                        .iter()
                        .filter(|snapshot| {
                            snapshot.account == account.email && snapshot.folder == folder
                        })
                        .flat_map(|snapshot| &snapshot.messages)
                        .find(|cached| {
                            cached.uid == message.uid
                                && cached.message_id == message.message_id
                                && cached.body_loaded
                        })
                    {
                        let header = message.clone();
                        *message = cached.clone();
                        message.uid_validity = header.uid_validity;
                        message.is_read = header.is_read;
                        message.read_revision = header.read_revision;
                        message.is_flagged = header.is_flagged;
                        message.flag_revision = header.flag_revision;
                        message.is_spam = header.is_spam;
                        message.is_draft = header.is_draft;
                    }
                }
                entry.action.batches.push(Batch {
                    account: account.clone(),
                    folder: folder.into(),
                    destination,
                    messages,
                });
            }
        }
        let unchanged = self
            .entries
            .iter()
            .filter(|entry| entry.requested && entry.pending.is_empty())
            .flat_map(|entry| {
                entry.snapshots.iter().map(|snapshot| Snapshot {
                    opened: snapshot.opened.clone(),
                    target: snapshot.target.clone(),
                    account: snapshot.account.clone(),
                    folder: snapshot.folder.clone(),
                    messages: snapshot
                        .messages
                        .iter()
                        .filter(|message| {
                            !entry.action.batches.iter().any(|batch| {
                                batch.account.email == snapshot.account
                                    && batch.folder == snapshot.folder
                                    && batch.messages.iter().any(|moved| {
                                        moved.uid == message.uid
                                            && moved.message_id == message.message_id
                                    })
                            })
                        })
                        .cloned()
                        .collect(),
                })
            })
            .collect();
        self.entries
            .retain(|entry| !entry.pending.is_empty() || !entry.action.batches.is_empty());
        unchanged
    }

    pub fn request(&mut self, id: Option<u64>) -> Option<u64> {
        let entry = self
            .entries
            .iter_mut()
            .rev()
            .find(|entry| !entry.requested && id.is_none_or(|id| entry.action.id == id))?;
        entry.requested = true;
        Some(entry.action.id)
    }

    pub fn take_ready(&mut self) -> Vec<Action> {
        let mut ready = Vec::new();
        self.entries.retain(|entry| {
            if entry.requested && entry.pending.is_empty() {
                ready.push(entry.action.clone());
                false
            } else {
                true
            }
        });
        ready.reverse();
        ready
    }

    pub fn retry(&mut self, action: Action) {
        if action.batches.is_empty() {
            return;
        }
        self.entries.push_back(Entry {
            action,
            pending: Vec::new(),
            requested: false,
            snapshots: Vec::new(),
        });
        self.entries
            .make_contiguous()
            .sort_by_key(|entry| entry.action.id);
        self.trim();
    }

    pub fn contains(&self, id: u64) -> bool {
        self.entries.iter().any(|entry| entry.action.id == id)
    }

    pub fn discard_empty(&mut self, id: u64) -> bool {
        self.entries.retain(|entry| {
            entry.action.id != id || !entry.pending.is_empty() || !entry.action.batches.is_empty()
        });
        self.contains(id)
    }

    fn trim(&mut self) {
        while self.entries.iter().filter(|entry| !entry.requested).count() > 10 {
            let oldest = self
                .entries
                .iter()
                .position(|entry| !entry.requested)
                .unwrap();
            self.entries.remove(oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account {
        Account {
            email: "account".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        }
    }

    fn messages(count: u32) -> Vec<Message> {
        (1..=count)
            .map(|uid| Message {
                uid,
                message_id: uid.to_string(),
                ..Default::default()
            })
            .collect()
    }

    fn complete(history: &mut History, id: u64, folder: &str, count: u32) {
        let target = ActionTarget::Sender(format!("sender-{id}-{folder}"));
        history.register(id, "account", folder, &target);
        history.complete(
            &account(),
            folder,
            &target,
            Some("Archive".into()),
            messages(count),
        );
    }

    #[test]
    fn completed_move_keeps_cached_body_and_uses_confirmed_server_flags() {
        let mut history = History::default();
        let id = history.begin(SenderAction::Archive);
        let target = ActionTarget::Messages(vec![(1, "one".into())]);
        let header = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            is_flagged: true,
            ..Default::default()
        };
        let cached = Message {
            body_loaded: true,
            body_text: "Cached GNOME message".into(),
            is_read: true,
            is_flagged: false,
            ..header.clone()
        };
        history.register(id, "account", "INBOX", &target);
        history.remember(
            id,
            Snapshot {
                opened: Vec::new(),
                target: target.clone(),
                account: "account".into(),
                folder: "INBOX".into(),
                messages: vec![cached],
            },
        );
        history.complete(
            &account(),
            "INBOX",
            &target,
            Some("Archive".into()),
            vec![header],
        );
        history.request(Some(id));
        let action = history.take_ready().pop().unwrap();
        let restored = &action.batches[0].messages[0];
        assert!(restored.body_loaded);
        assert_eq!(restored.body_text, "Cached GNOME message");
        assert!(!restored.is_read && restored.is_flagged);
    }

    #[test]
    fn restore_snapshots_exclude_failed_parts_of_a_bulk_action() {
        let mut history = History::default();
        let id = history.begin(SenderAction::Archive);
        let first = Message {
            uid: 1,
            message_id: "one".into(),
            sender: "first@example.com".into(),
            ..Default::default()
        };
        let second = Message {
            uid: 2,
            message_id: "two".into(),
            sender: "second@example.com".into(),
            ..Default::default()
        };
        let targets = [
            ActionTarget::Sender("first@example.com".into()),
            ActionTarget::Sender("second@example.com".into()),
        ];
        for (target, message) in targets.iter().zip([&first, &second]) {
            history.register(id, "account", "INBOX", target);
            history.remember(
                id,
                Snapshot {
                    target: target.clone(),
                    opened: vec![(message.uid, message.message_id.clone())],
                    account: "account".into(),
                    folder: "INBOX".into(),
                    messages: vec![message.clone()],
                },
            );
        }
        history.complete(&account(), "INBOX", &targets[0], None, Vec::new());
        history.complete(
            &account(),
            "INBOX",
            &targets[1],
            Some("Archive".into()),
            vec![second.clone()],
        );
        let snapshots = history.snapshots(id);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].target, targets[1]);
        assert_eq!(snapshots[0].messages, vec![second]);
        assert_eq!(snapshots[0].opened, vec![(2, "two".into())]);
    }

    #[test]
    fn keeps_ten_actions_regardless_of_bulk_message_count() {
        let mut history = History::default();
        for _ in 0..12 {
            let id = history.begin(SenderAction::Archive);
            complete(&mut history, id, "INBOX", 25);
        }
        assert_eq!(history.entries.len(), 10);
        for expected in (3..=12).rev() {
            assert_eq!(history.request(None), Some(expected));
            let action = history.take_ready().pop().unwrap();
            assert_eq!(action.id, expected);
            assert_eq!(action.batches[0].messages.len(), 25);
        }
        assert!(history.request(None).is_none());
    }

    #[test]
    fn bulk_undo_waits_for_every_folder_and_ignores_failed_parts() {
        let mut history = History::default();
        let id = history.begin(SenderAction::Delete);
        let first = ActionTarget::Sender("first".into());
        let second = ActionTarget::Sender("second".into());
        history.register(id, "account", "INBOX", &first);
        history.register(id, "account", "Work", &second);
        assert_eq!(history.request(Some(id)), Some(id));
        assert!(history.request(Some(id)).is_none());
        history.complete(
            &account(),
            "Work",
            &second,
            Some("Trash".into()),
            messages(30),
        );
        assert!(history.take_ready().is_empty());
        history.complete(&account(), "INBOX", &first, None, Vec::new());
        let action = history.take_ready().pop().unwrap();
        assert_eq!(action.id, id);
        assert_eq!(action.batches.len(), 1);
        assert_eq!(action.batches[0].folder, "Work");
        assert_eq!(action.batches[0].destination, "Trash");
    }

    #[test]
    fn failures_drop_empty_entries_and_partial_undo_can_retry() {
        let mut history = History::default();
        let failed = history.begin(SenderAction::Delete);
        let target = ActionTarget::Sender("first".into());
        history.register(failed, "account", "INBOX", &target);
        history.complete(&account(), "INBOX", &target, None, Vec::new());
        assert!(history.request(None).is_none());
        let id = history.begin(SenderAction::Archive);
        complete(&mut history, id, "INBOX", 3);
        history.request(None);
        let mut action = history.take_ready().pop().unwrap();
        action.batches[0].messages.remove(0);
        history.retry(action);
        assert_eq!(history.request(None), Some(id));
        assert_eq!(history.take_ready()[0].batches[0].messages.len(), 2);
    }

    #[test]
    fn new_actions_do_not_evict_an_already_requested_pending_undo() {
        let mut history = History::default();
        let id = history.begin(SenderAction::Archive);
        let target = ActionTarget::Sender("first".into());
        history.register(id, "account", "INBOX", &target);
        history.request(Some(id));
        for _ in 0..12 {
            let next = history.begin(SenderAction::Delete);
            complete(&mut history, next, "Work", 1);
        }
        history.complete(
            &account(),
            "INBOX",
            &target,
            Some("Archive".into()),
            messages(1),
        );
        assert_eq!(history.take_ready()[0].id, id);
        assert_eq!(history.entries.len(), 10);
    }
}
