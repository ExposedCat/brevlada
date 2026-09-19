use super::*;

impl State {
    pub(super) fn load_previews(&self) {
        if self.selected_sender.borrow().is_none() {
            return;
        }
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        for message in self
            .thread_groups
            .borrow()
            .iter()
            .filter_map(|group| group.first())
        {
            if message.body_loaded
                || self.pending.borrow().contains(&message.uid)
                || !self.preview_pending.borrow_mut().insert(message.uid)
            {
                continue;
            }
            self.sender.body(BodyRequest {
                account: account.clone(),
                folder: self.folder.borrow().clone(),
                uid: message.uid,
                generation: self.generation.get(),
                selection: self.selection.get(),
                mark_read: false,
            });
        }
    }

    pub(super) fn preview_loaded(&self, preview: Message) {
        self.preview_pending.borrow_mut().remove(&preview.uid);
        self.apply_cached_body(preview);
    }

    pub(super) fn apply_cached_body(&self, preview: Message) {
        let message = self
            .messages
            .borrow()
            .iter()
            .find(|m| m.uid == preview.uid && m.message_id == preview.message_id)
            .cloned();
        if let Some(mut message) = message {
            message.body_text = preview.body_text;
            message.body_html = preview.body_html;
            message.body_loaded = preview.body_loaded;
            message.attachments = preview.attachments;
            self.update_body(&message);
            if self
                .cards
                .borrow()
                .get(&message.uid)
                .is_some_and(|card| card.is_expanded())
            {
                self.open(message.uid);
            }
        }
    }
}
