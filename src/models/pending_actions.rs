use super::{Message, action_target::ActionTarget, sender_action::SenderAction};
use std::collections::HashMap;

#[derive(Default)]
pub struct PendingActions(HashMap<(String, String, ActionTarget), SenderAction>);

impl PendingActions {
    pub fn begin(
        &mut self,
        account: &str,
        folder: &str,
        target: impl Into<ActionTarget>,
        action: SenderAction,
    ) -> bool {
        use std::collections::hash_map::Entry;
        match self.0.entry((account.into(), folder.into(), target.into())) {
            Entry::Vacant(entry) => {
                entry.insert(action);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    pub fn finish(&mut self, account: &str, folder: &str, target: impl Into<ActionTarget>) {
        self.0
            .remove(&(account.into(), folder.into(), target.into()));
    }

    pub fn project(&self, account: &str, folder: &str, message: &Message) -> Option<Message> {
        let mut visible = message.clone();
        for ((pending_account, pending_folder, target), action) in &self.0 {
            if pending_account == account && pending_folder == folder && target.matches(message) {
                if *action == SenderAction::MarkRead {
                    visible.is_read = true;
                } else {
                    return None;
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
            assert!(pending.begin("a", "INBOX", "sender@example.com", action));
            assert!(pending.project("a", "INBOX", &message).is_none());
            assert!(pending.project("b", "INBOX", &message).is_some());
            pending.finish("a", "INBOX", "sender@example.com");
            assert!(pending.project("a", "INBOX", &message).is_some());
        }
    }
}
