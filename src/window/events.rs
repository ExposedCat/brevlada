use super::*;

impl State {
    pub(super) fn event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Unsubscribe {
                account,
                folder,
                uid,
                message_id,
                result,
            } => {
                let done = matches!(&result, Ok(crate::backend::unsubscribe::Outcome::Done));
                if self
                    .account
                    .borrow()
                    .as_ref()
                    .is_some_and(|current| current.email == account)
                {
                    if *self.folder.borrow() == folder
                        && let Some(card) = self.cards.borrow().get(&uid)
                        && self
                            .open_group
                            .borrow()
                            .iter()
                            .any(|message| message.uid == uid && message.message_id == message_id)
                    {
                        card.finish_unsubscribe(done);
                    }
                    if let Some(card) = self.sent_cards.borrow().get(&message_id) {
                        card.finish_unsubscribe(done);
                    }
                }
                match result {
                    Ok(crate::backend::unsubscribe::Outcome::Open(url)) => ui::links::open(&url),
                    Ok(crate::backend::unsubscribe::Outcome::Done) => {
                        self.toast.add_toast(adw::Toast::new("Unsubscribed"))
                    }
                    Err(error) => self
                        .toast
                        .add_toast(adw::Toast::new(&format!("Could not unsubscribe: {error}"))),
                }
            }
            Event::Composed(request, result) => self.composer_finished(request, result),
            Event::DraftDeleted(request, result) => self.draft_deleted(request, result),
            Event::SentCacheChanged(account) => {
                if self
                    .account
                    .borrow()
                    .as_ref()
                    .is_some_and(|current| current.email == account)
                {
                    self.request_related_sent();
                }
            }
            Event::RelatedSent(generation, selection, mut messages)
                if generation == self.generation.get() && selection == self.selection.get() =>
            {
                self.reconcile_drafts(&mut messages);
                for item in &mut messages {
                    if !item.message.body_loaded
                        && let Some(loaded) = self.related_sent.borrow().iter().find(|old| {
                            old.folder == item.folder
                                && old.message.uid == item.message.uid
                                && old.message.message_id == item.message.message_id
                                && old.message.body_loaded
                        })
                    {
                        item.message = loaded.message.clone();
                    }
                }
                if *self.related_sent.borrow() != messages || self.cards.borrow().is_empty() {
                    let only_drafts_changed = self
                        .related_sent
                        .borrow()
                        .iter()
                        .filter(|item| !item.message.is_draft)
                        .eq(messages.iter().filter(|item| !item.message.is_draft));
                    *self.related_sent.borrow_mut() = messages;
                    ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                        if only_drafts_changed {
                            self.update_draft_widgets();
                        } else {
                            self.render_conversation();
                        }
                    });
                }
            }
            Event::SentBody(generation, selection, folder, uid, result)
                if generation == self.generation.get() && selection == self.selection.get() =>
            {
                let id = self
                    .related_sent
                    .borrow()
                    .iter()
                    .find(|item| item.folder == folder && item.message.uid == uid)
                    .map(|item| item.message.message_id.clone());
                if let Some(id) = id {
                    self.sent_pending.borrow_mut().remove(&id);
                    match result {
                        Ok(message) => {
                            if let Some(item) = self
                                .related_sent
                                .borrow_mut()
                                .iter_mut()
                                .find(|item| item.folder == folder && item.message.uid == uid)
                            {
                                item.message = message.clone();
                            }
                            if let Some(card) = self.sent_cards.borrow().get(&id) {
                                card.update(&message);
                            }
                            if message.is_draft {
                                self.draft_editor(&folder, &message);
                            }
                        }
                        Err(error) => {
                            if let Some(card) = self.sent_cards.borrow().get(&id) {
                                card.error(&error);
                            }
                            self.draft_body_error(&folder, &id, &error);
                        }
                    }
                }
            }
            Event::SenderActionFinished {
                account,
                folder,
                sender,
                mut messages,
                error,
                removed,
            } => {
                self.sender_actions
                    .borrow_mut()
                    .confirm_removed(&account, &folder, &removed);
                if let Some(messages) = &mut messages {
                    if let Some(previous) = self
                        .folder_cache
                        .borrow()
                        .get(&(account.clone(), folder.clone()))
                    {
                        models::read_state::merge(messages, previous.iter());
                    }
                    self.folder_cache
                        .borrow_mut()
                        .insert((account.clone(), folder.clone()), messages.clone());
                }
                self.sender_actions
                    .borrow_mut()
                    .finish(&account, &folder, &sender);
                if self
                    .account
                    .borrow()
                    .as_ref()
                    .is_some_and(|current| current.email == account)
                    && *self.folder.borrow() == folder
                {
                    self.sender_pane.borrow_mut().remove(&removed);
                    if let Some(messages) = messages {
                        self.event(Event::Messages(
                            self.generation.get(),
                            messages,
                            self.loading.get(),
                        ));
                    } else {
                        self.refresh_sender_view();
                    }
                    self.advance_message();
                    self.load_expanded();
                }
                if let Some(error) = error {
                    self.toast.add_toast(adw::Toast::new(&format!(
                        "Action for {}: {error}",
                        sender.label()
                    )));
                }
            }
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
                self.merge_read_states(&mut messages);
                self.loading.set(pending);
                self.refresh.set_sensitive(!pending);
                ui::states::refreshing(&self.refresh, pending);
                {
                    let existing = self.messages.borrow();
                    let sender_messages = self.sender_pane.borrow();
                    let by_id: HashMap<_, _> = existing
                        .iter()
                        .chain(sender_messages.messages())
                        .filter(|message| message.body_loaded)
                        .map(|message| ((message.uid, message.message_id.as_str()), message))
                        .collect();
                    for message in &mut messages {
                        if !message.body_loaded
                            && let Some(existing) =
                                by_id.get(&(message.uid, message.message_id.as_str()))
                        {
                            message.body_loaded = true;
                            message.body_html = existing.body_html.clone();
                            message.body_text = existing.body_text.clone();
                            message.attachments = existing.attachments.clone();
                            message.inline_media = existing.inline_media.clone();
                            message.inline_media_loaded = existing.inline_media_loaded;
                            message.parcels = existing.parcels.clone();
                            message.unsubscribe =
                                message.unsubscribe.clone().or(existing.unsubscribe.clone());
                        }
                    }
                }
                if (!pending || !messages.is_empty())
                    && let Some(account) = self.account.borrow().as_ref()
                {
                    self.folder_cache.borrow_mut().insert(
                        (account.email.clone(), self.folder.borrow().clone()),
                        messages.clone(),
                    );
                }
                *self.messages.borrow_mut() = messages;
                if self.sender_pane.borrow().sender().is_some() {
                    let folder_messages = self.messages.borrow();
                    for sender_message in self.sender_pane.borrow_mut().messages_mut().iter_mut() {
                        if let Some(updated) = folder_messages.iter().find(|message| {
                            message.uid == sender_message.uid
                                && message.message_id == sender_message.message_id
                        }) {
                            *sender_message = updated.clone();
                        }
                    }
                }
                self.render_list();
                self.load_expanded();
            }
            Event::SenderPage(generation, sender_generation, mut messages, has_more)
                if generation == self.generation.get()
                    && self.sender_pane.borrow().accepts(sender_generation) =>
            {
                self.merge_read_states(&mut messages);
                self.sender_pane
                    .borrow_mut()
                    .finish(sender_generation, messages, has_more);
                self.thread_load_more.set_visible(false);
                self.thread_load_more.set_sensitive(true);
                self.render_sender_pane();
                self.advance_message();
            }
            Event::SenderPageError(generation, sender_generation, error)
                if generation == self.generation.get()
                    && self.sender_pane.borrow().accepts(sender_generation) =>
            {
                self.sender_pane.borrow_mut().fail(sender_generation);
                self.thread_load_more.set_visible(true);
                self.thread_load_more.set_sensitive(true);
                if self.sender_pane.borrow().messages().is_empty() {
                    ui::states::list_state(
                        &self.thread_stack,
                        "Could not load messages",
                        false,
                        false,
                    );
                }
                self.toast.add_toast(adw::Toast::new(&format!(
                    "Could not load sender messages: {error}"
                )));
            }
            Event::CacheList(email, folder, mut messages) => {
                let current = self
                    .account
                    .borrow()
                    .as_ref()
                    .is_some_and(|account| account.email == email)
                    && *self.folder.borrow() == folder;
                if current {
                    if !self.loading.get() {
                        self.event(Event::Messages(self.generation.get(), messages, false));
                    }
                } else {
                    if let Some(previous) = self
                        .folder_cache
                        .borrow()
                        .get(&(email.clone(), folder.clone()))
                    {
                        models::read_state::merge(&mut messages, previous.iter());
                    }
                    self.folder_cache
                        .borrow_mut()
                        .insert((email, folder), messages);
                }
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
                    .is_some_and(|card| card.is_expanded() || card.is_reply_pending())
                {
                    self.open(uid);
                }
            }
            Event::Body(generation, selection, message)
                if generation == self.generation.get() && selection == self.selection.get() =>
            {
                if message.is_read {
                    self.pending.borrow_mut().remove(&message.uid);
                }
                self.update_body(&message);
                if message.is_draft {
                    self.draft_editor(&self.folder.borrow(), &message);
                }
                if message.is_read {
                    self.load_expanded();
                }
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
                    || self
                        .sender_pane
                        .borrow()
                        .messages()
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
