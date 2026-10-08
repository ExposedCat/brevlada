use super::*;

impl State {
    pub(super) fn request_preview(&self, message: &Message) {
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        if message.body_loaded
            || message.list_preview.is_some()
            || self.search_sender.borrow().as_ref().is_some_and(|search| {
                search.messages.iter().any(|source| {
                    source.uid == message.uid
                        && source.message_id == message.message_id
                        && source.body_loaded
                })
            })
            || self
                .sender_pane
                .borrow()
                .messages()
                .iter()
                .chain(self.messages.borrow().iter())
                .any(|source| {
                    source.uid == message.uid
                        && source.message_id == message.message_id
                        && source.body_loaded
                })
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
        let in_search = self.search_sender.borrow().as_ref().is_some_and(|search| {
            search.messages.iter().any(|message| {
                message.uid == preview.uid && message.message_id == preview.message_id
            })
        });
        let opened =
            self.open_group.borrow().iter().any(|message| {
                message.uid == preview.uid && message.message_id == preview.message_id
            });
        if !in_sender_pane && !opened && !in_search {
            let changed = if let Some(header) =
                self.messages.borrow_mut().iter_mut().find(|message| {
                    message.uid == preview.uid && message.message_id == preview.message_id
                }) {
                let changed = header.body_text != preview.body_text
                    || header.body_html != preview.body_html
                    || header.recipients != preview.recipients
                    || header.cc != preview.cc
                    || header.reply_to != preview.reply_to
                    || header.calendar_events != preview.calendar_events
                    || header.attachments != preview.attachments
                    || header.attachment_details != preview.attachment_details
                    || header.parcels != preview.parcels
                    || header.tickets != preview.tickets;
                header.list_preview = preview.list_preview;
                header.display_prepared = preview.display_prepared;
                header.remote_media = preview.remote_media;
                header.body_text = preview.body_text;
                header.body_html = preview.body_html;
                header.recipients = preview.recipients;
                header.cc = preview.cc;
                header.reply_to = preview.reply_to;
                header.calendar_events = preview.calendar_events;
                header.attachments = preview.attachments;
                header.attachment_details = preview.attachment_details;
                header.parcels = preview.parcels;
                header.tickets = preview.tickets;
                changed
            } else {
                false
            };
            if changed {
                *self.search_snapshots.borrow_mut() = [None, None];
            }
            if changed
                && (!self.search.text().trim().is_empty()
                    || !self.thread_search.text().trim().is_empty())
            {
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
        let message = message
            .or_else(|| {
                self.sender_pane
                    .borrow()
                    .messages()
                    .iter()
                    .find(|m| m.uid == preview.uid && m.message_id == preview.message_id)
                    .cloned()
            })
            .or_else(|| {
                self.open_group
                    .borrow()
                    .iter()
                    .find(|m| m.uid == preview.uid && m.message_id == preview.message_id)
                    .cloned()
            })
            .or_else(|| {
                self.search_sender.borrow().as_ref().and_then(|search| {
                    search
                        .messages
                        .iter()
                        .find(|message| {
                            message.uid == preview.uid && message.message_id == preview.message_id
                        })
                        .cloned()
                })
            });
        if let Some(mut message) = message {
            message.reply_to = preview.reply_to;
            message.cc = preview.cc;
            message.list_preview = preview.list_preview;
            message.display_prepared = preview.display_prepared;
            message.remote_media = preview.remote_media;
            message.body_text = preview.body_text;
            message.body_html = preview.body_html;
            message.body_loaded = preview.body_loaded;
            message.attachments = preview.attachments;
            message.attachment_details = preview.attachment_details;
            message.inline_media = preview.inline_media;
            message.inline_media_loaded = preview.inline_media_loaded;
            message.parcels = preview.parcels;
            message.calendar_events = preview.calendar_events;
            message.tickets = preview.tickets;
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
