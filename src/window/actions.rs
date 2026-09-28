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
        if self.messages.borrow().is_empty() && !self.has_cached_folder() {
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
        // Preserve body/read updates made since the last list response.
        if self.has_cached_folder() {
            let current = self.account.borrow();
            let current = current.as_ref().unwrap();
            self.folder_cache.borrow_mut().insert(
                (current.email.clone(), self.folder.borrow().clone()),
                self.messages.borrow().clone(),
            );
        }
        let cached = self
            .folder_cache
            .borrow()
            .get(&(account.email.clone(), folder.clone()))
            .cloned()
            .unwrap_or_default();
        self.sender.focus(&account.email, &folder);
        self.generation.set(self.generation.get() + 1);
        self.new_selection();
        *self.account.borrow_mut() = Some(account);
        self.reset_list();
        *self.selected_sender.borrow_mut() = None;
        self.back.set_visible(false);
        self.thread_sidebar.set_visible(false);
        self.restore_accounts_on_back.set(false);
        self.reset_threads();
        *self.folder.borrow_mut() = folder;
        self.selected.borrow_mut().clear();
        self.deferred_read_sort.borrow_mut().clear();
        self.cards.borrow_mut().clear();
        self.sent_cards.borrow_mut().clear();
        self.related_sent.borrow_mut().clear();
        self.sent_pending.borrow_mut().clear();
        self.open_group.borrow_mut().clear();
        *self.messages.borrow_mut() = cached;
        self.loading.set(true);
        self.render_list();
        ui::states::select_message(&self.viewer);
        self.load();
    }

    pub(super) fn has_cached_folder(&self) -> bool {
        self.account.borrow().as_ref().is_some_and(|account| {
            self.folder_cache
                .borrow()
                .contains_key(&(account.email.clone(), self.folder.borrow().clone()))
        })
    }

    fn reset_list(&self) {
        self.rendering.set(true);
        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        self.groups.borrow_mut().clear();
        self.rendering.set(false);
    }

    fn reset_threads(&self) {
        self.rendering.set(true);
        while let Some(row) = self.thread_list.first_child() {
            self.thread_list.remove(&row);
        }
        self.thread_groups.borrow_mut().clear();
        self.rendering.set(false);
    }

    pub(super) fn filter_sender(&self, message: Option<Message>) {
        let opening = message.is_some() && !self.thread_sidebar.get_visible();
        if message.is_some() {
            self.reset_threads();
        }
        *self.selected_sender.borrow_mut() = message.as_ref().map(models::senders::key);
        if opening {
            let was_expanded = self.account_sidebar.get_visible();
            self.account_sidebar.set_visible(false);
            self.restore_accounts_on_back.set(was_expanded);
        } else if message.is_none() && self.restore_accounts_on_back.replace(false) {
            self.account_sidebar.set_visible(true);
        }
        self.thread_sidebar.set_visible(message.is_some());
        self.back.set_visible(message.is_some());
        self.render_list();
        self.thread_scroll.vadjustment().set_value(0.0);
    }

    pub(super) fn show_thread(self: &Rc<Self>, group: Vec<Message>) {
        self.new_selection();
        let selected: Vec<_> = group.iter().map(|message| message.uid).collect();
        if *self.selected.borrow() != selected {
            *self.deferred_read_sort.borrow_mut() = group
                .iter()
                .filter(|message| !message.is_read)
                .map(|message| message.uid)
                .collect();
        }
        *self.selected.borrow_mut() = selected;
        *self.open_group.borrow_mut() = group.clone();
        self.related_sent.borrow_mut().clear();
        self.sent_pending.borrow_mut().clear();
        self.cards.borrow_mut().clear();
        self.sent_cards.borrow_mut().clear();
        ui::states::loading_thread(&self.viewer);
        self.viewer_scroll.vadjustment().set_value(0.0);
        self.request_related_sent();
        self.render_list();
        self.viewer_reveal.play();
    }

    pub(super) fn request_related_sent(self: &Rc<Self>) {
        if self.open_group.borrow().is_empty() {
            return;
        }
        if let Some(account) = self.account.borrow().clone() {
            if let Err(error) = self.sender.send(Command::RelatedSent {
                account,
                folder: self.folder.borrow().clone(),
                messages: self.open_group.borrow().clone(),
                generation: self.generation.get(),
                selection: self.selection.get(),
            }) {
                self.toast.add_toast(adw::Toast::new(&error.to_string()));
                self.render_conversation();
            }
        }
    }

    pub(super) fn render_conversation(self: &Rc<Self>) {
        let group: Vec<_> = self
            .open_group
            .borrow()
            .iter()
            .map(|original| {
                self.messages
                    .borrow()
                    .iter()
                    .find(|current| {
                        current.uid == original.uid && current.message_id == original.message_id
                    })
                    .cloned()
                    .unwrap_or_else(|| original.clone())
            })
            .collect();
        let mut messages: Vec<_> = group
            .iter()
            .cloned()
            .map(|message| (false, String::new(), message))
            .collect();
        messages.extend(
            self.related_sent
                .borrow()
                .iter()
                .map(|item| (true, item.folder.clone(), item.message.clone())),
        );
        messages.sort_by_key(|(_, _, message)| std::cmp::Reverse((message.timestamp, message.uid)));
        let previous = self.cards.borrow().clone();
        let previous_sent = self.sent_cards.borrow().clone();
        ui::clear(&self.viewer);
        self.cards.borrow_mut().clear();
        self.sent_cards.borrow_mut().clear();
        self.viewer.set_vexpand(false);
        let unread = group.iter().any(|message| !message.is_read);
        for (index, (sent, folder, message)) in messages.iter().enumerate() {
            let hide_quotes = models::conversation::quoted_message_visible(
                message,
                messages[index + 1..].iter().map(|(_, _, older)| older),
            );
            let expanded = if *sent {
                previous_sent
                    .get(&message.message_id)
                    .map(|card| card.is_expanded())
            } else {
                previous.get(&message.uid).map(|card| card.is_expanded())
            }
            .unwrap_or((!*sent && !message.is_read) || (!unread && index == 0));
            let card = self.card(
                message,
                expanded,
                if *sent { Some(folder) } else { None },
                messages.len() > 1,
                hide_quotes,
            );
            self.viewer.append(&card.widget);
            if *sent {
                self.sent_cards
                    .borrow_mut()
                    .insert(message.message_id.clone(), card);
                if expanded && !message.body_loaded {
                    self.open_sent(folder, message.uid, &message.message_id);
                }
            } else {
                self.cards.borrow_mut().insert(message.uid, card);
            }
            if expanded && !sent {
                self.open(message.uid);
            }
        }
    }

    pub(super) fn card(
        self: &Rc<Self>,
        message: &Message,
        expanded: bool,
        sent_folder: Option<&str>,
        threaded: bool,
        hide_quotes: bool,
    ) -> ui::viewer::Card {
        let weak = Rc::downgrade(self);
        let uid = message.uid;
        let sent_folder = sent_folder.map(str::to_owned);
        let sent_id = message.message_id.clone();
        let reply_state = Rc::downgrade(self);
        let media_state = Rc::downgrade(self);
        let mut media_key = self.media_key(message);
        if let Some(folder) = &sent_folder {
            media_key.1 = folder.clone();
        }
        let downloaded = self.downloaded_media.borrow().contains(&media_key);
        let sender = models::senders::key(message);
        let own = self
            .account
            .borrow()
            .as_ref()
            .is_some_and(|account| sender == models::senders::address(&account.email));
        let trusted = own || self.is_trusted(&sender);
        ui::viewer::card(
            message,
            expanded,
            threaded,
            sent_folder.is_some(),
            hide_quotes,
            &self.avatars,
            downloaded,
            trusted,
            move || {
                if let Some(state) = weak.upgrade() {
                    if let Some(folder) = &sent_folder {
                        state.open_sent(folder, uid, &sent_id);
                    } else {
                        state.open(uid);
                    }
                }
            },
            move |reply_message| {
                if let Some(state) = reply_state.upgrade() {
                    let recipient = if own {
                        reply_message.recipients.clone()
                    } else {
                        models::senders::identity(reply_message).1
                    };
                    state.compose.reply(reply_message, &recipient);
                    let adjustment = state.viewer_scroll.vadjustment();
                    adjustment.set_value(adjustment.lower());
                }
            },
            move || {
                if let Some(state) = media_state.upgrade() {
                    state.mark_media_downloaded(media_key.clone());
                }
            },
        )
    }

    pub(super) fn open_sent(&self, folder: &str, uid: u32, id: &str) {
        let Some(item) = self
            .related_sent
            .borrow()
            .iter()
            .find(|item| item.message.message_id == id)
            .cloned()
        else {
            return;
        };
        if item.message.body_loaded
            || folder.is_empty()
            || !self.sent_pending.borrow_mut().insert(id.to_owned())
        {
            return;
        }
        if let Some(account) = self.account.borrow().clone() {
            self.send(Command::SentBody {
                account,
                folder: folder.to_owned(),
                uid,
                generation: self.generation.get(),
                selection: self.selection.get(),
            });
        }
    }

    pub(super) fn new_selection(&self) {
        let selection = self.selection.get() + 1;
        self.selection.set(selection);
        self.sender.select(selection);
        self.pending.borrow_mut().clear();
        self.preview_pending.borrow_mut().clear();
        self.sent_pending.borrow_mut().clear();
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
