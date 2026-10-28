use super::*;

impl State {
    pub(super) fn cache_pruned(self: &Rc<Self>, account: &str, folder: &str, removed: &[Message]) {
        self.sender_actions
            .borrow_mut()
            .confirm_removed(account, folder, removed);
        let keep = |message: &Message| {
            !self
                .sender_actions
                .borrow()
                .is_removed(account, folder, message)
        };
        if let Some(cached) = self
            .folder_cache
            .borrow_mut()
            .get_mut(&(account.to_owned(), folder.to_owned()))
        {
            cached.retain(&keep);
        }
        self.draft_local
            .borrow_mut()
            .retain(|(email, source, _), local| {
                email != account || source != folder || keep(&local.item.message)
            });
        self.refresh_search_cache(account, folder);
        if !self
            .account
            .borrow()
            .as_ref()
            .is_some_and(|current| current.email == account)
        {
            return;
        }
        let before = (
            self.open_group.borrow().len(),
            self.related_sent.borrow().len(),
        );
        self.related_sent
            .borrow_mut()
            .retain(|item| item.folder != folder || keep(&item.message));
        if *self.folder.borrow() == folder {
            self.messages.borrow_mut().retain(&keep);
            self.sender_pane.borrow_mut().remove(removed);
            let expired: Vec<_> = self
                .open_group
                .borrow()
                .iter()
                .filter(|message| !keep(message))
                .map(|message| message.uid)
                .collect();
            self.open_group.borrow_mut().retain(&keep);
            *self.selected.borrow_mut() = self
                .open_group
                .borrow()
                .iter()
                .map(|message| message.uid)
                .collect();
            for uid in expired {
                self.pending.borrow_mut().remove(&uid);
                self.preview_pending.borrow_mut().remove(&uid);
                self.deferred_read_sort.borrow_mut().remove(&uid);
                self.sender
                    .body_handled(self.selection.get(), folder, uid, false);
            }
            if let Some(search) = self.search_sender.borrow_mut().as_mut() {
                search.messages.retain(&keep);
            }
            self.render_list();
        }
        let after = (
            self.open_group.borrow().len(),
            self.related_sent.borrow().len(),
        );
        if before != after {
            for message in removed {
                self.sent_pending.borrow_mut().remove(&message.message_id);
                self.sender
                    .body_handled(self.selection.get(), folder, message.uid, true);
            }
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                if after == (0, 0) {
                    self.cards.borrow_mut().clear();
                    self.sent_cards.borrow_mut().clear();
                    ui::states::select_message(&self.viewer);
                } else {
                    self.render_conversation();
                }
            });
            self.refresh_open_styles();
        }
    }
}
