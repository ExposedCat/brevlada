use super::*;

impl State {
    pub(super) fn event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Accounts(accounts) => {
                ui::clear(&self.sidebar);
                self.folder_boxes.borrow_mut().clear();
                if accounts.is_empty() {
                    ui::shell::no_accounts(&self.sidebar);
                }
                for account in accounts {
                    let weak = Rc::downgrade(self);
                    let selected = account.clone();
                    let discover_state = Rc::downgrade(self);
                    let discovery = account.clone();
                    let row = ui::sidebar::AccountRow::new(
                        &account,
                        self.navigation_selection.clone(),
                        move || {
                            if let Some(state) = weak.upgrade() {
                                state.select(selected.clone(), "INBOX".into());
                            }
                        },
                        move || {
                            if let Some(state) = discover_state.upgrade() {
                                state.send(Command::Folders(discovery.clone()));
                            }
                        },
                    );
                    self.sidebar.append(&row.widget);
                    self.folder_boxes
                        .borrow_mut()
                        .insert(account.email.clone(), (account, row.folders));
                }
            }
            Event::Folders(email, folders) => {
                if let Some((account, container)) = self.folder_boxes.borrow().get(&email) {
                    ui::clear(container);
                    let weak = Rc::downgrade(self);
                    let account = account.clone();
                    ui::folders::populate(
                        container,
                        folders,
                        self.navigation_selection.clone(),
                        move |folder| {
                            if let Some(state) = weak.upgrade() {
                                state.select(account.clone(), folder);
                            }
                        },
                    );
                }
            }
            Event::Messages(generation, mut messages, pending)
                if generation == self.generation.get() =>
            {
                self.loading.set(pending);
                self.refresh.set_sensitive(!pending);
                ui::states::refreshing(&self.refresh, pending);
                for message in &mut messages {
                    if !message.body_loaded
                        && let Some(existing) = self.messages.borrow().iter().find(|old| {
                            old.uid == message.uid
                                && old.message_id == message.message_id
                                && old.body_loaded
                        })
                    {
                        message.body_loaded = true;
                        message.body_html = existing.body_html.clone();
                        message.body_text = existing.body_text.clone();
                        message.attachments = existing.attachments.clone();
                    }
                }
                *self.messages.borrow_mut() = messages;
                self.render_list();
                self.load_expanded();
            }
            Event::Body(generation, selection, message)
                if generation == self.generation.get() && selection == self.selection.get() =>
            {
                self.pending.borrow_mut().remove(&message.uid);
                self.update_body(&message);
            }
            Event::BodyError(generation, selection, uid, error)
                if generation == self.generation.get() && selection == self.selection.get() =>
            {
                self.pending.borrow_mut().remove(&uid);
                if let Some(card) = self.cards.borrow().get(&uid) {
                    card.error(&error);
                }
                if self
                    .messages
                    .borrow()
                    .iter()
                    .any(|m| m.uid == uid && m.body_loaded)
                {
                    self.toast.add_toast(adw::Toast::new(&error));
                }
            }
            Event::Error(generation, error)
                if generation.is_none() || generation == Some(self.generation.get()) =>
            {
                if generation.is_some() {
                    self.loading.set(false);
                    self.refresh.set_sensitive(self.account.borrow().is_some());
                }
                ui::states::refreshing(&self.refresh, false);
                if self.messages.borrow().is_empty() {
                    ui::states::list_state(
                        &self.list_stack,
                        &format!("Failed to load messages: {error}"),
                        false,
                        true,
                    );
                } else {
                    self.toast.add_toast(adw::Toast::new(&error));
                }
            }
            _ => {}
        }
    }
}
