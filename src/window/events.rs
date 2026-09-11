use super::*;

impl State {
    pub(super) fn event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Accounts(accounts) => {
                self.sync.set_sensitive(!accounts.is_empty());
                ui::clear(&self.sidebar);
                self.folder_boxes.borrow_mut().clear();
                self.folder_names.borrow_mut().clear();
                self.unread.borrow_mut().clear();
                if accounts.is_empty() {
                    ui::shell::no_accounts(&self.sidebar);
                }
                for account in accounts {
                    self.sender.register(
                        account.clone(),
                        self.expansion.for_account(&account.email).is_expanded(""),
                    );
                    let unread = ui::sidebar::Unread::default();
                    let weak = Rc::downgrade(self);
                    let selected = account.clone();
                    let discover_state = Rc::downgrade(self);
                    let discovery = account.clone();
                    let row = ui::sidebar::AccountRow::new(
                        &account,
                        &unread,
                        &self.expansion.for_account(&account.email),
                        &self.avatars,
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
                    let weak = Rc::downgrade(self);
                    let email = account.email.clone();
                    row.folders.connect_visible_notify(move |folders| {
                        if let Some(state) = weak.upgrade() {
                            state.sender.expanded(&email, folders.get_visible());
                        }
                    });
                    self.unread
                        .borrow_mut()
                        .insert(account.email.clone(), unread);
                    self.folder_boxes
                        .borrow_mut()
                        .insert(account.email.clone(), (account, row.folders));
                }
            }
            Event::SidebarReady => {
                self.sender.sync();
            }
            Event::Folders(email, folders) => {
                if self.folder_names.borrow().get(&email) == Some(&folders) {
                    return;
                }
                self.folder_names
                    .borrow_mut()
                    .insert(email.clone(), folders.clone());
                if let Some((account, container)) = self.folder_boxes.borrow().get(&email) {
                    container.remove_css_class("folders-loading");
                    ui::clear(container);
                    let weak = Rc::downgrade(self);
                    let account = account.clone();
                    ui::folders::populate(
                        container,
                        folders,
                        &self.unread.borrow()[&email],
                        &self.expansion.for_account(&email),
                        self.navigation_selection.clone(),
                        move |folder| {
                            if let Some(state) = weak.upgrade() {
                                state.select(account.clone(), folder);
                            }
                        },
                    );
                    if container.first_child().is_none() {
                        container.append(&ui::label("No folders", "dim-label"));
                    }
                }
            }
            Event::Unread(email, folders) => {
                if let Some(unread) = self.unread.borrow().get(&email) {
                    unread.update(folders);
                }
            }
            Event::UnreadSnapshot(email, folders) => {
                if let Some(unread) = self.unread.borrow().get(&email) {
                    unread.replace(folders);
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
            Event::CacheList(email, folder, messages)
                if self
                    .account
                    .borrow()
                    .as_ref()
                    .is_some_and(|account| account.email == email)
                    && *self.folder.borrow() == folder
                    && !self.loading.get() =>
            {
                self.event(Event::Messages(self.generation.get(), messages, false));
            }
            Event::CacheBody(email, folder, message)
                if self
                    .account
                    .borrow()
                    .as_ref()
                    .is_some_and(|account| account.email == email)
                    && *self.folder.borrow() == folder =>
            {
                self.apply_cached_body(message);
            }
            Event::Preview(generation, selection, message)
                if generation == self.generation.get() && selection == self.selection.get() =>
            {
                self.preview_loaded(message);
            }
            Event::PreviewError(generation, selection, uid)
                if generation == self.generation.get() && selection == self.selection.get() =>
            {
                self.preview_pending.borrow_mut().remove(&uid);
                if self
                    .cards
                    .borrow()
                    .get(&uid)
                    .is_some_and(|card| card.is_expanded())
                {
                    self.open(uid);
                }
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
            Event::Avatar(email, image) => self.avatars.resolved(&email, image),
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
