use super::*;

impl State {
    pub(super) fn refresh_open_styles(&self) {
        self.active_sender_row
            .set_key(self.sender_pane.borrow().sender().map(str::to_owned));
        let opened = self.open_group.borrow();
        let key = (0..ui::virtual_list::model(&self.thread_list).n_items())
            .filter_map(|position| ui::virtual_list::item(&self.thread_list, position))
            .find(|row| overlaps(&row.messages, &opened))
            .map(|row| row.key);
        self.active_thread_row.set_key(key);
    }

    pub(super) fn render_sender_pane(&self) {
        self.rendering.set(true);
        self.render_pane(true);
        self.rendering.set(false);
        self.maybe_load_sender_page();
    }
    pub(super) fn render_list(&self) {
        self.rendering.set(true);
        self.render_pane(false);
        if self.sender_pane.borrow().sender().is_some() {
            self.render_pane(true);
        }
        self.rendering.set(false);
    }

    fn render_pane(&self, threads: bool) {
        let (list, list_scroll, list_stack) = if threads {
            (&self.thread_list, &self.thread_scroll, &self.thread_stack)
        } else {
            (&self.list, &self.list_scroll, &self.list_stack)
        };
        let selected_group = ui::virtual_list::selected(list)
            .and_then(|index| ui::virtual_list::item(list, index).map(|row| row.messages));
        let sender = if threads {
            self.sender_pane.borrow().sender().map(str::to_owned)
        } else {
            None
        };
        let query = self.search.text().trim().to_lowercase();
        let messages = if threads {
            self.sender_pane
                .borrow()
                .messages()
                .iter()
                .filter_map(|message| self.visible_message(message))
                .collect()
        } else {
            self.visible_messages()
        };
        let mut groups = if let Some(sender) = &sender {
            let own = self
                .account
                .borrow()
                .as_ref()
                .map(|account| account.email.to_lowercase());
            let messages: Vec<_> = messages
                .into_iter()
                .filter(|message| {
                    let key = models::senders::key(message);
                    key == *sender || own.as_deref() == Some(key.as_str())
                })
                .collect();
            let mut groups = models::threads(&messages, &query);
            groups.retain(|group| {
                group
                    .iter()
                    .any(|message| models::senders::key(message) == *sender)
            });
            groups
        } else {
            models::senders::groups(&messages, &query)
        };
        // Keep the active unread conversation in its unread sort position until
        // another conversation opens, while still displaying its real read state.
        let deferred = self.deferred_read_sort.borrow();
        let unread_first = if threads {
            self.thread_unread_first.unread_first()
        } else {
            self.sender_unread_first.unread_first()
        };
        groups.sort_by_key(|group| {
            std::cmp::Reverse((
                unread_first
                    && group
                        .iter()
                        .any(|message| !message.is_read || deferred.contains(&message.uid)),
                group[0].timestamp,
                group[0].uid,
            ))
        });
        if !threads {
            self.has_more_senders
                .set(query.is_empty() && groups.len() > self.visible_limit.get());
            if query.is_empty() {
                groups.truncate(self.visible_limit.get());
            }
        }
        let related = |left: &[Message], right: &[Message]| {
            if sender.is_some() {
                overlaps(left, right)
            } else {
                models::senders::key(&left[0]) == models::senders::key(&right[0])
            }
        };
        let selected = selected_group
            .as_ref()
            .and_then(|previous| groups.iter().position(|group| related(previous, group)));
        let keys: Vec<String> = groups
            .iter()
            .map(|group| {
                if sender.is_some() {
                    thread_key(group)
                } else {
                    models::senders::key(&group[0])
                }
            })
            .collect();
        let changed: Vec<usize> = {
            let previous = ui::virtual_list::groups(list);
            let by_key: HashMap<String, &Vec<Message>> = previous
                .iter()
                .map(|group| {
                    let key = if threads {
                        thread_key(group)
                    } else {
                        models::senders::key(&group[0])
                    };
                    (key, group)
                })
                .collect();
            groups
                .iter()
                .enumerate()
                .filter_map(|(index, group)| {
                    by_key
                        .get(&keys[index])
                        .is_some_and(|old| !same_row(old, group, threads))
                        .then_some(index)
                })
                .collect()
        };
        let count = groups.len();
        let rows: Vec<_> = keys
            .into_iter()
            .zip(groups)
            .map(|(key, messages)| ui::virtual_list::Row { key, messages })
            .collect();
        ui::scroll_position::preserve_offset(list_scroll, || {
            ui::virtual_list::update(list, &rows, &changed, selected);
        });
        self.refresh_open_styles();
        if count == 0 {
            let loading = if threads {
                self.sender_pane.borrow().loading()
            } else {
                self.loading.get() && !self.has_cached_folder()
            };
            if !threads && !loading && !query.is_empty() {
                ui::states::no_senders_found(list_stack);
                return;
            }
            ui::states::list_state(
                list_stack,
                if loading {
                    "Loading messages..."
                } else if !query.is_empty() {
                    "No matching messages"
                } else if sender.is_some() {
                    "No messages from this sender"
                } else {
                    "No messages in this folder"
                },
                loading,
                false,
            );
        } else {
            list_stack.set_visible_child_name("list");
        }
    }

    pub(super) fn update_body(self: &Rc<Self>, message: &Message) {
        let mut message = message.clone();
        self.merge_read_states(std::slice::from_mut(&mut message));
        if message.body_loaded && message.parcels.is_empty() && !message.body_html.is_empty() {
            message.parcels = models::parcel::parse_message(&message);
        }
        let old_parcels_empty = self
            .messages
            .borrow()
            .iter()
            .find(|m| m.uid == message.uid && m.message_id == message.message_id)
            .map(|old| old.parcels.is_empty())
            .or_else(|| {
                self.sender_pane
                    .borrow()
                    .messages()
                    .iter()
                    .find(|m| m.uid == message.uid && m.message_id == message.message_id)
                    .map(|old| old.parcels.is_empty())
            })
            .unwrap_or(false);
        let has_new_parcel = !message.parcels.is_empty() && old_parcels_empty;
        if let Some(existing) = self
            .messages
            .borrow_mut()
            .iter_mut()
            .find(|m| m.uid == message.uid && m.message_id == message.message_id)
        {
            *existing = message.clone();
        }
        if let Some(existing) = self
            .sender_pane
            .borrow_mut()
            .messages_mut()
            .iter_mut()
            .find(|m| m.uid == message.uid && m.message_id == message.message_id)
        {
            *existing = message.clone();
        }
        if let Some(existing) = self
            .open_group
            .borrow_mut()
            .iter_mut()
            .find(|m| m.uid == message.uid && m.message_id == message.message_id)
        {
            *existing = message.clone();
        }
        let Some(visible) = self.visible_message(&message) else {
            return;
        };
        let message = &visible;
        self.render_list();
        if let Some(card) = self.cards.borrow().get(&message.uid) {
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                card.update(message)
            });
        }
        if has_new_parcel {
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                self.render_conversation();
            });
        }
    }
}

fn thread_key(group: &[Message]) -> String {
    let message = group.iter().min_by_key(|message| message.uid).unwrap();
    format!("thread:{}:{}", message.uid, message.message_id)
}

fn same_row(left: &[Message], right: &[Message], threads: bool) -> bool {
    let unread = |group: &[Message]| group.iter().any(|message| !message.is_read);
    if unread(left) != unread(right) {
        return false;
    }
    if threads {
        let same_message = match (left.first(), right.first()) {
            (Some(a), Some(b)) => {
                a.uid == b.uid
                    && a.uid_validity == b.uid_validity
                    && a.message_id == b.message_id
                    && a.subject == b.subject
                    && a.body_text == b.body_text
                    && a.body_html == b.body_html
                    && a.timestamp == b.timestamp
            }
            (None, None) => true,
            _ => false,
        };
        left.len() == right.len()
            && same_message
            && left.iter().any(|message| message.is_flagged)
                == right.iter().any(|message| message.is_flagged)
            && left
                .iter()
                .max_by_key(|message| message.timestamp)
                .map(|message| message.timestamp)
                == right
                    .iter()
                    .max_by_key(|message| message.timestamp)
                    .map(|message| message.timestamp)
    } else {
        fn latest(group: &[Message]) -> Option<&Message> {
            group
                .iter()
                .max_by_key(|message| (message.timestamp, message.uid))
        }
        match (latest(left), latest(right)) {
            (Some(a), Some(b)) => {
                a.uid == b.uid
                    && a.sender == b.sender
                    && a.subject == b.subject
                    && a.timestamp == b.timestamp
                    && a.date == b.date
            }
            (None, None) => true,
            _ => false,
        }
    }
}

pub(super) fn overlaps(left: &[Message], right: &[Message]) -> bool {
    left.iter().any(|a| {
        right
            .iter()
            .any(|b| a.uid == b.uid && a.message_id == b.message_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_revisions_do_not_replace_visually_unchanged_threads() {
        let message = Message {
            uid: 1,
            is_read: true,
            read_revision: 1,
            ..Default::default()
        };
        let updated = Message {
            read_revision: 2,
            ..message.clone()
        };
        assert!(same_row(
            std::slice::from_ref(&message),
            std::slice::from_ref(&updated),
            true
        ));
        assert!(!same_row(
            &[message],
            &[Message {
                is_read: false,
                ..updated
            }],
            true
        ));
    }

    #[test]
    fn older_mail_does_not_replace_an_unchanged_sender_row() {
        let latest = Message {
            uid: 2,
            timestamp: 2,
            sender: "a@example.com".into(),
            subject: "Latest".into(),
            is_read: true,
            ..Default::default()
        };
        let older = Message {
            uid: 1,
            timestamp: 1,
            sender: latest.sender.clone(),
            subject: "Older".into(),
            is_read: true,
            ..Default::default()
        };
        assert!(same_row(
            &[latest.clone()],
            &[latest.clone(), older.clone()],
            false
        ));
        assert!(!same_row(&[latest.clone()], &[latest, older], true));
    }
}
