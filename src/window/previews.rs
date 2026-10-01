use super::*;

impl State {
    pub(super) fn request_preview(&self, message: &Message) {
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        if message.body_loaded
            || self.pending.borrow().contains(&message.uid)
            || !self.preview_pending.borrow_mut().insert(message.uid)
        {
            return;
        }
        self.sender.body(BodyRequest {
            account,
            folder: self.folder.borrow().clone(),
            uid: message.uid,
            generation: self.generation.get(),
            selection: self.selection.get(),
            mark_read: false,
        });
    }

    pub(super) fn preview_loaded(self: &Rc<Self>, preview: Message) {
        self.preview_pending.borrow_mut().remove(&preview.uid);
        self.apply_cached_body(preview);
    }

    pub(super) fn apply_cached_body(self: &Rc<Self>, preview: Message) {
        let in_sender_pane =
            self.sender_pane.borrow().messages().iter().any(|message| {
                message.uid == preview.uid && message.message_id == preview.message_id
            });
        let opened =
            self.open_group.borrow().iter().any(|message| {
                message.uid == preview.uid && message.message_id == preview.message_id
            });
        if !in_sender_pane && !opened {
            let changed = if let Some(header) =
                self.messages.borrow_mut().iter_mut().find(|message| {
                    message.uid == preview.uid && message.message_id == preview.message_id
                }) {
                let changed = header.body_text != preview.body_text;
                header.body_text = preview.body_text;
                changed
            } else {
                false
            };
            if changed && !self.search.text().trim().is_empty() {
                self.render_list();
            }
            return;
        }
        let message = self
            .messages
            .borrow()
            .iter()
            .find(|m| m.uid == preview.uid && m.message_id == preview.message_id)
            .cloned();
        let message = message.or_else(|| {
            self.sender_pane
                .borrow()
                .messages()
                .iter()
                .find(|m| m.uid == preview.uid && m.message_id == preview.message_id)
                .cloned()
        });
        if let Some(mut message) = message {
            message.body_text = preview.body_text;
            message.body_html = preview.body_html;
            message.body_loaded = preview.body_loaded;
            message.attachments = preview.attachments;
            message.inline_media = preview.inline_media;
            message.inline_media_loaded = preview.inline_media_loaded;
            message.parcels = preview.parcels;
            message.unsubscribe = preview.unsubscribe.or(message.unsubscribe);
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
