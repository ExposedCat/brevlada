use super::{Message, action_target::ActionTarget, sender_action::SenderAction};
use std::collections::{HashMap, HashSet};

type RemovedMessages = HashMap<(u32, String), HashSet<Option<u32>>>;

struct RetainedSpam {
    message: Message,
    undo_pending: bool,
}

type SpamKey = (String, String, u32, String);

#[derive(Default)]
pub struct PendingActions {
    pending: HashMap<(String, String, ActionTarget), SenderAction>,
    removed: HashMap<(String, String), RemovedMessages>,
    spam: HashMap<SpamKey, RetainedSpam>,
    restored: HashMap<SpamKey, Message>,
}

impl PendingActions {
    pub fn begin(
        &mut self,
        account: &str,
        folder: &str,
        target: impl Into<ActionTarget>,
        action: SenderAction,
    ) -> bool {
        use std::collections::hash_map::Entry;
        match self
            .pending
            .entry((account.into(), folder.into(), target.into()))
        {
            Entry::Vacant(entry) => {
                entry.insert(action);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    pub fn finish(
        &mut self,
        account: &str,
        folder: &str,
        target: impl Into<ActionTarget>,
    ) -> Option<SenderAction> {
        self.pending
            .remove(&(account.into(), folder.into(), target.into()))
    }

    pub fn retain_spam(&mut self, account: &str, folder: &str, messages: &[Message]) {
        for message in messages {
            self.restored.remove(&(
                account.into(),
                folder.into(),
                message.uid,
                message.message_id.clone(),
            ));
            self.spam.insert(
                (
                    account.into(),
                    folder.into(),
                    message.uid,
                    message.message_id.clone(),
                ),
                RetainedSpam {
                    message: message.clone(),
                    undo_pending: false,
                },
            );
        }
    }

    pub fn finish_spam(
        &mut self,
        account: &str,
        folder: &str,
        target: &ActionTarget,
        moved: &[Message],
    ) {
        self.spam.retain(|(a, f, _, _), retained| {
            a != account
                || f != folder
                || !target.matches(&retained.message)
                || moved.iter().any(|message| {
                    message.uid == retained.message.uid
                        && message.message_id == retained.message.message_id
                })
        });
    }

    pub fn undo_spam(
        &mut self,
        account: &str,
        folder: &str,
        target: &ActionTarget,
    ) -> Vec<Message> {
        self.spam
            .iter_mut()
            .filter_map(|((a, f, _, _), retained)| {
                if a == account
                    && f == folder
                    && !retained.undo_pending
                    && target.matches(&retained.message)
                {
                    retained.undo_pending = true;
                    Some(retained.message.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn finish_undo_spam(
        &mut self,
        account: &str,
        folder: &str,
        originals: &[Message],
        restored: &[Message],
    ) {
        self.spam.retain(|(a, f, _, _), retained| {
            if a != account
                || f != folder
                || !originals.iter().any(|message| {
                    message.uid == retained.message.uid
                        && message.message_id == retained.message.message_id
                })
            {
                return true;
            }
            retained.undo_pending = false;
            !restored.iter().any(|message| {
                message.uid == retained.message.uid
                    && message.message_id == retained.message.message_id
            })
        });
    }

    pub fn hold_restored(&mut self, account: &str, folder: &str, message: &Message) {
        self.restored.insert(
            (
                account.into(),
                folder.into(),
                message.uid,
                message.message_id.clone(),
            ),
            message.clone(),
        );
    }

    pub fn dismiss_spam(&mut self, account: &str, folder: &str, sender: &str) -> bool {
        let mut changed = false;
        self.spam.retain(|(a, f, _, _), retained| {
            let dismiss =
                a == account && f == folder && super::senders::key(&retained.message) == sender;
            changed |= dismiss;
            !dismiss
        });
        self.restored.retain(|(a, f, _, _), message| {
            a != account || f != folder || super::senders::key(message) != sender
        });
        changed
    }

    pub fn is_retained_spam(&self, account: &str, folder: &str, message: &Message) -> bool {
        self.spam
            .get(&(
                account.into(),
                folder.into(),
                message.uid,
                message.message_id.clone(),
            ))
            .is_some_and(|retained| {
                retained.message.uid_validity.is_none()
                    || message.uid_validity.is_none()
                    || retained.message.uid_validity == message.uid_validity
            })
    }

    pub fn visible(&self, account: &str, folder: &str, messages: &[Message]) -> Vec<Message> {
        let mut visible: Vec<_> = messages
            .iter()
            .filter_map(|message| self.project(account, folder, message))
            .collect();
        for ((a, f, _, _), retained) in &self.spam {
            if a == account
                && f == folder
                && !messages.iter().any(|message| {
                    message.uid == retained.message.uid
                        && message.message_id == retained.message.message_id
                })
                && let Some(message) = self.project(account, folder, &retained.message)
            {
                visible.push(message);
            }
        }
        for ((a, f, _, _), restored) in &self.restored {
            if a == account
                && f == folder
                && !visible.iter().any(|message| {
                    message.uid == restored.uid && message.message_id == restored.message_id
                })
                && let Some(message) = self.project(account, folder, restored)
            {
                visible.push(message);
            }
        }
        visible
    }

    pub fn confirm_removed(&mut self, account: &str, folder: &str, messages: &[Message]) {
        let removed = self
            .removed
            .entry((account.into(), folder.into()))
            .or_default();
        for message in messages {
            removed
                .entry((message.uid, message.message_id.clone()))
                .or_default()
                .insert(message.uid_validity);
        }
    }

    pub fn project(&self, account: &str, folder: &str, message: &Message) -> Option<Message> {
        let retained = self.spam.get(&(
            account.into(),
            folder.into(),
            message.uid,
            message.message_id.clone(),
        ));
        let retained = retained.filter(|retained| {
            retained.message.uid_validity.is_none()
                || message.uid_validity.is_none()
                || retained.message.uid_validity == message.uid_validity
        });
        let keep = retained.is_some();
        if !keep
            && self
                .removed
                .get(&(account.into(), folder.into()))
                .is_some_and(|removed| {
                    removed
                        .get(&(message.uid, message.message_id.clone()))
                        .is_some_and(|validities| {
                            message.uid_validity.is_none()
                                || validities.contains(&message.uid_validity)
                        })
                })
        {
            return None;
        }
        let mut visible = message.clone();
        if let Some(retained) = retained {
            visible.is_spam = !retained.undo_pending;
        }
        for ((pending_account, pending_folder, target), action) in &self.pending {
            if pending_account == account && pending_folder == folder && target.matches(message) {
                if *action == SenderAction::Spam && keep {
                    continue;
                }
                if let Some(read) = action.read_state() {
                    visible.is_read = read;
                } else {
                    visible.is_flagged = action.flagged_state()?;
                }
            }
        }
        Some(visible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spam_stays_visible_through_confirmation_and_refresh_until_sender_reopens() {
        let message = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            sender: "sender@example.com".into(),
            is_flagged: true,
            ..Default::default()
        };
        let mut pending = PendingActions::default();
        let target = ActionTarget::Messages(vec![(1, "one".into())]);
        pending.retain_spam("a", "INBOX", std::slice::from_ref(&message));
        pending.begin("a", "INBOX", &target, SenderAction::Spam);
        assert!(pending.project("a", "INBOX", &message).unwrap().is_spam);
        assert!(!pending.project("b", "INBOX", &message).unwrap().is_spam);
        assert!(!pending.project("a", "Work", &message).unwrap().is_spam);
        pending.finish("a", "INBOX", &target);
        pending.finish_spam("a", "INBOX", &target, std::slice::from_ref(&message));
        pending.confirm_removed("a", "INBOX", std::slice::from_ref(&message));
        let retained = pending.visible("a", "INBOX", &[]);
        assert_eq!(retained.len(), 1);
        assert!(retained[0].is_spam && retained[0].is_flagged);
        let replaced = Message {
            uid_validity: Some(8),
            ..message.clone()
        };
        assert!(!pending.project("a", "INBOX", &replaced).unwrap().is_spam);
        assert!(!pending.dismiss_spam("a", "INBOX", "other@example.com"));
        assert!(pending.dismiss_spam("a", "INBOX", "sender@example.com"));
        assert!(pending.visible("a", "INBOX", &[]).is_empty());
        assert!(pending.project("a", "INBOX", &message).is_none());
    }

    #[test]
    fn spam_can_be_undone_while_pending_and_retries_after_a_failed_undo() {
        let message = Message {
            uid: 1,
            message_id: "one".into(),
            sender: "sender@example.com".into(),
            ..Default::default()
        };
        let target = ActionTarget::Messages(vec![(1, "one".into())]);
        let mut pending = PendingActions::default();
        pending.retain_spam("a", "INBOX", std::slice::from_ref(&message));
        pending.begin("a", "INBOX", &target, SenderAction::Spam);
        let undo = pending.undo_spam("a", "INBOX", &target);
        assert_eq!(undo, vec![message.clone()]);
        assert!(!pending.project("a", "INBOX", &message).unwrap().is_spam);
        assert!(pending.undo_spam("a", "INBOX", &target).is_empty());
        pending.finish("a", "INBOX", &target);
        pending.finish_spam("a", "INBOX", &target, &undo);
        pending.confirm_removed("a", "INBOX", &undo);
        pending.finish_undo_spam("a", "INBOX", &undo, &[]);
        assert!(pending.visible("a", "INBOX", &[])[0].is_spam);
        let undo = pending.undo_spam("a", "INBOX", &target);
        pending.finish_undo_spam("a", "INBOX", &undo, &undo);
        assert!(pending.visible("a", "INBOX", &[]).is_empty());

        pending.retain_spam("a", "INBOX", std::slice::from_ref(&message));
        pending.begin("a", "INBOX", &target, SenderAction::Spam);
        pending.dismiss_spam("a", "INBOX", "sender@example.com");
        pending.finish("a", "INBOX", &target);
        pending.finish_spam("a", "INBOX", &target, std::slice::from_ref(&message));
        assert!(pending.visible("a", "INBOX", &[]).is_empty());
    }

    #[test]
    fn unread_and_star_projections_are_reversible_and_keep_messages_visible() {
        let message = Message {
            uid: 1,
            is_read: true,
            sender: "sender@example.com".into(),
            ..Default::default()
        };
        let mut pending = PendingActions::default();
        for action in [
            SenderAction::MarkUnread,
            SenderAction::Star,
            SenderAction::Unstar,
        ] {
            pending.begin("a", "INBOX", "sender@example.com", action.clone());
            let visible = pending.project("a", "INBOX", &message).unwrap();
            assert_eq!(visible.is_read, action != SenderAction::MarkUnread);
            assert_eq!(visible.is_flagged, action == SenderAction::Star);
            assert_eq!(
                pending.project("b", "INBOX", &message),
                Some(message.clone())
            );
            pending.finish("a", "INBOX", "sender@example.com");
            assert_eq!(
                pending.project("a", "INBOX", &message),
                Some(message.clone())
            );
        }
    }

    #[test]
    fn completed_removals_reject_late_snapshots_without_hiding_new_mail() {
        let old = Message {
            uid: 1,
            uid_validity: Some(7),
            message_id: "one".into(),
            ..Default::default()
        };
        let mut pending = PendingActions::default();
        pending.confirm_removed("a", "INBOX", std::slice::from_ref(&old));
        assert!(pending.project("a", "INBOX", &old).is_none());
        assert!(
            pending
                .project(
                    "a",
                    "INBOX",
                    &Message {
                        uid_validity: None,
                        ..old.clone()
                    }
                )
                .is_none()
        );
        for replacement in [
            Message {
                uid_validity: Some(8),
                ..old.clone()
            },
            Message {
                message_id: "new".into(),
                ..old.clone()
            },
            Message {
                uid: 2,
                ..old.clone()
            },
        ] {
            assert!(pending.project("a", "INBOX", &replacement).is_some());
        }
        assert!(pending.project("b", "INBOX", &old).is_some());
        assert!(pending.project("a", "Archive", &old).is_some());
    }

    #[test]
    fn message_targets_do_not_affect_other_conversations_or_reused_uids() {
        let first = Message {
            uid: 1,
            message_id: "first".into(),
            sender: "same@example.com".into(),
            ..Default::default()
        };
        let other = Message {
            uid: 2,
            message_id: "other".into(),
            ..first.clone()
        };
        let replaced = Message {
            message_id: "replacement".into(),
            ..first.clone()
        };
        let mut pending = PendingActions::default();
        let target = ActionTarget::Messages(vec![(1, "first".into())]);
        pending.begin("a", "INBOX", &target, SenderAction::Delete);
        assert!(pending.project("a", "INBOX", &first).is_none());
        assert_eq!(pending.project("a", "INBOX", &other), Some(other));
        assert_eq!(pending.project("a", "INBOX", &replaced), Some(replaced));
        pending.finish("a", "INBOX", &target);
        assert_eq!(pending.project("a", "INBOX", &first), Some(first));
    }

    #[test]
    fn pending_changes_survive_stale_refreshes_and_are_scoped_and_reversible() {
        let message = Message {
            sender: "Sender <sender@example.com>".into(),
            ..Default::default()
        };
        let mut pending = PendingActions::default();
        assert!(pending.begin("a", "INBOX", "sender@example.com", SenderAction::MarkRead));
        assert!(!pending.begin("a", "INBOX", "sender@example.com", SenderAction::Delete));
        assert!(pending.project("a", "INBOX", &message).unwrap().is_read);
        assert!(!pending.project("b", "INBOX", &message).unwrap().is_read);
        assert!(!pending.project("a", "Archive", &message).unwrap().is_read);
        pending.finish("a", "INBOX", "sender@example.com");
        assert!(!pending.project("a", "INBOX", &message).unwrap().is_read);
        for action in [
            SenderAction::Delete,
            SenderAction::Archive,
            SenderAction::Spam,
        ] {
            assert!(pending.begin("a", "INBOX", "sender@example.com", action.clone()));
            assert!(pending.project("a", "INBOX", &message).is_none());
            assert!(pending.project("b", "INBOX", &message).is_some());
            pending.finish("a", "INBOX", "sender@example.com");
            assert!(pending.project("a", "INBOX", &message).is_some());
        }
    }
}
