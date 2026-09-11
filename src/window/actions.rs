use super::*;

impl State {
    pub(super) fn send(&self, command: Command) {
        if self.sender.send(command).is_err() {
            self.toast.add_toast(adw::Toast::new(
                "Mail service stopped. Restart Brevlada to retry.",
            ));
        }
    }

    pub(super) fn load(self: &Rc<Self>) {
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        let generation = self.generation.get();
        self.loading.set(true);
        self.refresh.set_sensitive(false);
        ui::states::refreshing(&self.refresh, true);
        if self.messages.borrow().is_empty() {
            ui::states::list_state(&self.list_stack, "Loading messages...", true, false);
        }
        self.send(Command::Load {
            account,
            folder: self.folder.borrow().clone(),
            generation,
        });
        self.load_expanded();
    }

    pub(super) fn select(self: &Rc<Self>, account: Account, folder: String) {
        self.sender.focus(&account.email, &folder);
        self.generation.set(self.generation.get() + 1);
        self.new_selection();
        *self.account.borrow_mut() = Some(account);
        self.reset_list();
        *self.selected_sender.borrow_mut() = None;
        self.back.set_visible(false);
        *self.folder.borrow_mut() = folder;
        self.selected.borrow_mut().clear();
        self.cards.borrow_mut().clear();
        self.messages.borrow_mut().clear();
        self.render_list();
        ui::states::select_message(&self.viewer);
        self.load();
    }

    fn reset_list(&self) {
        self.rendering.set(true);
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        self.groups.borrow_mut().clear();
        self.rendering.set(false);
    }

    pub(super) fn filter_sender(&self, message: Option<Message>) {
        if message.is_some() {
            self.new_selection();
            self.selected.borrow_mut().clear();
            self.cards.borrow_mut().clear();
            ui::states::select_message(&self.viewer);
        }
        self.reset_list();
        *self.selected_sender.borrow_mut() = message.as_ref().map(models::senders::key);
        self.back.set_visible(message.is_some());
        self.render_list();
        self.list_scroll.vadjustment().set_value(0.0);
    }

    pub(super) fn show_thread(self: &Rc<Self>, group: Vec<Message>) {
        self.new_selection();
        ui::clear(&self.viewer);
        self.viewer_scroll.vadjustment().set_value(0.0);
        self.cards.borrow_mut().clear();
        *self.selected.borrow_mut() = group.iter().map(|m| m.uid).collect();
        self.viewer.set_vexpand(false);
        let unread = group.iter().any(|message| !message.is_read);
        for (index, message) in group.iter().enumerate() {
            let expanded = !message.is_read || (!unread && index == 0);
            let card = self.card(message, expanded);
            self.viewer.append(&card.widget);
            self.cards.borrow_mut().insert(message.uid, card);
            if expanded {
                self.open(message.uid);
            }
        }
        self.load_previews();
    }

    pub(super) fn card(self: &Rc<Self>, message: &Message, expanded: bool) -> ui::viewer::Card {
        let weak = Rc::downgrade(self);
        let uid = message.uid;
        let reply_state = Rc::downgrade(self);
        ui::viewer::card(
            message,
            expanded,
            self.selected.borrow().len() > 1,
            &self.avatars,
            move || {
                if let Some(state) = weak.upgrade() {
                    state.open(uid);
                }
            },
            move |reply_message| {
                if let Some(state) = reply_state.upgrade() {
                    state.compose.reply(reply_message);
                    let adjustment = state.viewer_scroll.vadjustment();
                    adjustment.set_value(adjustment.lower());
                }
            },
        )
    }

    pub(super) fn new_selection(&self) {
        let selection = self.selection.get() + 1;
        self.selection.set(selection);
        self.sender.select(selection);
        self.pending.borrow_mut().clear();
        self.preview_pending.borrow_mut().clear();
    }

    pub(super) fn load_expanded(&self) {
        let uids: Vec<_> = self
            .cards
            .borrow()
            .iter()
            .filter(|(_, card)| card.is_expanded())
            .map(|(uid, _)| *uid)
            .collect();
        for uid in uids {
            self.open(uid);
        }
    }

    pub(super) fn open(&self, uid: u32) {
        let message = self
            .messages
            .borrow()
            .iter()
            .find(|message| message.uid == uid)
            .cloned();
        let Some(message) = message else {
            return;
        };
        if message.body_loaded && message.is_read {
            return;
        }
        if self.preview_pending.borrow().contains(&uid) {
            return;
        }
        if !self.pending.borrow_mut().insert(uid) {
            return;
        }
        if let Some(card) = self.cards.borrow().get(&uid) {
            card.loading();
        }
        if let Some(account) = self.account.borrow().clone() {
            self.sender.body(BodyRequest {
                account,
                folder: self.folder.borrow().clone(),
                uid,
                generation: self.generation.get(),
                selection: self.selection.get(),
                mark_read: true,
            });
        }
    }
}
