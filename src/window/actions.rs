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
        self.generation.set(self.generation.get() + 1);
        self.new_selection();
        self.visited
            .borrow_mut()
            .insert(account.email.clone(), account.clone());
        *self.account.borrow_mut() = Some(account);
        self.list_title.set_label(&folder);
        *self.folder.borrow_mut() = folder;
        self.selected.borrow_mut().clear();
        self.cards.borrow_mut().clear();
        self.messages.borrow_mut().clear();
        self.render_list();
        ui::states::select_message(&self.viewer);
        self.content_title.set_title("Online Accounts");
        self.load();
    }

    pub(super) fn show_thread(self: &Rc<Self>, group: Vec<Message>) {
        self.new_selection();
        ui::clear(&self.viewer);
        self.viewer_scroll.vadjustment().set_value(0.0);
        self.cards.borrow_mut().clear();
        *self.selected.borrow_mut() = group.iter().map(|m| m.uid).collect();
        if let Some(message) = group.first() {
            self.content_title.set_title(&ui::subject(message));
        }
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
    }

    pub(super) fn card(self: &Rc<Self>, message: &Message, expanded: bool) -> ui::viewer::Card {
        let weak = Rc::downgrade(self);
        let uid = message.uid;
        ui::viewer::card(
            message,
            expanded,
            self.selected.borrow().len() > 1,
            move || {
                if let Some(state) = weak.upgrade() {
                    state.open(uid);
                }
            },
        )
    }

    pub(super) fn new_selection(&self) {
        let selection = self.selection.get() + 1;
        self.selection.set(selection);
        self.sender.select(selection);
        self.pending.borrow_mut().clear();
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
            });
        }
    }
}
