use super::*;

impl State {
    pub(super) fn refresh_open_styles(&self) {
        let sender_key = self.sender_pane.borrow().sender().map(|sender| {
            self.account
                .borrow()
                .as_ref()
                .map(|account| {
                    crate::backend::search::Location {
                        account: account.email.clone(),
                        folder: self.folder.borrow().clone(),
                    }
                    .key(sender)
                })
                .filter(|key| self.search_locations.borrow().contains_key(key))
                .unwrap_or_else(|| sender.to_owned())
        });
        self.active_sender_row.set_key(sender_key);
        let opened = self.open_group.borrow();
        let identities: HashSet<_> = opened
            .iter()
            .map(|message| (message.uid, message.message_id.as_str()))
            .collect();
        let key = (0..ui::virtual_list::model(&self.thread_list).n_items()).find_map(|position| {
            ui::virtual_list::with_item(&self.thread_list, position, |row| {
                row.messages
                    .iter()
                    .any(|message| identities.contains(&(message.uid, message.message_id.as_str())))
                    .then(|| row.key.clone())
            })
            .flatten()
        });
        self.active_thread_row.set_key(key);
    }

    pub(super) fn render_sender_pane(&self) {
        self.search_snapshots.borrow_mut()[1] = None;
        if self.search_sender.borrow().is_some() && self.sender_search_active() {
            self.search_snapshots.borrow_mut()[0] = None;
            self.render_or_queue_search(false);
        }
        self.rendering.set(true);
        self.render_or_queue_search(true);
        self.rendering.set(false);
        self.maybe_load_sender_page();
    }
    pub(super) fn render_list(&self) {
        *self.search_snapshots.borrow_mut() = [None, None];
        self.rendering.set(true);
        self.render_or_queue_search(false);
        if self.sender_pane.borrow().sender().is_some() {
            self.render_or_queue_search(true);
        }
        self.rendering.set(false);
    }

    pub(super) fn render_pane(&self, threads: bool) {
        self.render_pane_with_progress(threads, true);
    }

    pub(super) fn render_pane_with_progress(&self, threads: bool, show_progress: bool) {
        let sender = if threads {
            self.sender_pane.borrow().sender().map(str::to_owned)
        } else {
            None
        };
        let entry = if threads {
            &self.thread_search
        } else {
            &self.search
        };
        let query = if self.search_open(threads) {
            entry.text().trim().to_lowercase()
        } else {
            String::new()
        };
        let filtered = !threads && self.sender_search_active();
        if filtered && query.is_empty() {
            self.cancel_search(false);
            self.has_more_senders.set(false);
            self.search_locations.borrow_mut().clear();
            ui::virtual_list::replace(&self.list, 0, None);
            ui::states::list_state(&self.list_stack, "Enter a search query", false, false);
            if let Some(search) = self.search_sender.borrow_mut().as_mut() {
                search.messages.clear();
            }
            if self.search_sender.borrow().is_some() {
                self.cancel_search(true);
                self.render_or_queue_search(true);
            }
            return;
        }
        if filtered && let Err(error) = self.search_filters.filters() {
            self.cancel_search(false);
            self.has_more_senders.set(false);
            ui::states::list_state(&self.list_stack, error, false, false);
            return;
        }
        if (filtered || !query.is_empty()) && (!threads || sender.is_some()) {
            let cached = self.search_snapshots.borrow()[0].clone();
            let messages = cached.unwrap_or_else(|| {
                // Both searches share one snapshot. Subsequent keystrokes copy
                // no message bodies; normalization and filtering stay on the worker.
                let mut messages: HashMap<_, _> = self
                    .search_documents(&self.messages.borrow())
                    .into_iter()
                    .map(|message| ((message.uid, message.message_id.clone()), message))
                    .collect();
                if self.search_sender.borrow().is_some() {
                    for message in self.search_documents(self.sender_pane.borrow().messages()) {
                        messages.insert((message.uid, message.message_id.clone()), message);
                    }
                }
                let messages = std::sync::Arc::new(messages.into_values().collect::<Vec<_>>());
                self.search_snapshots.borrow_mut()[0] = Some(messages.clone());
                messages
            });
            let mut request = crate::backend::search::Request::new(
                threads,
                query,
                sender,
                self.account
                    .borrow()
                    .as_ref()
                    .map(|account| account.email.to_lowercase()),
                messages,
            );
            if !threads {
                request.filters = self.search_filters.filters().unwrap();
                let account = self.account.borrow();
                let mut accounts: Vec<_> = self
                    .search_filters
                    .accounts_for(account.as_ref().map(|account| account.email.as_str()))
                    .into_iter()
                    .collect();
                accounts.sort();
                request.scope = Some(crate::backend::search::Scope {
                    current: account
                        .as_ref()
                        .filter(|_| !self.loading.get() || self.has_cached_folder())
                        .map(|account| crate::backend::search::Location {
                            account: account.email.clone(),
                            folder: self.folder.borrow().clone(),
                        }),
                    folder: self.search_filters.folder(),
                    accounts,
                    cache_path: glib::user_data_dir().join("brevlada/emails.db"),
                    revision: self.search_cache_revision.get(),
                });
            }
            request.unread_first = if threads {
                self.thread_unread_first.unread_first()
            } else {
                self.sender_unread_first.unread_first()
            };
            request.deferred_read = self.deferred_read_sort.borrow().clone();
            if threads {
                let cached = self.search_snapshots.borrow()[1].clone();
                let sender_messages = cached.unwrap_or_else(|| {
                    let documents = if let Some(matches) = self.matched_sender_messages() {
                        self.search_documents(&matches)
                    } else {
                        self.search_documents(self.sender_pane.borrow().messages())
                    };
                    let messages = std::sync::Arc::new(documents);
                    self.search_snapshots.borrow_mut()[1] = Some(messages.clone());
                    messages
                });
                if self.search_sender.borrow().is_some() {
                    request.messages = sender_messages;
                } else {
                    request.sender_messages = Some(sender_messages);
                }
            }
            self.begin_search(threads, show_progress);
            self.search_worker.submit(request);
            return;
        }
        self.cancel_search(threads);
        let messages = if threads {
            if let Some(matches) = self.matched_sender_messages() {
                self.visible_list_group(&matches, true)
            } else {
                self.visible_list_group(self.sender_pane.borrow().messages(), true)
            }
        } else {
            self.visible_list_group(&self.messages.borrow(), false)
        };
        let groups = if let Some(sender) = &sender {
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
            let mut groups = models::threads(&messages, "");
            groups.retain(|group| {
                group
                    .iter()
                    .any(|message| models::senders::key(message) == *sender)
            });
            groups
        } else {
            models::senders::groups(&messages, "")
        };
        self.publish_search_groups(threads, &query, groups, Vec::new());
    }

    pub(super) fn publish_search_groups(
        &self,
        threads: bool,
        query: &str,
        mut groups: Vec<Vec<Message>>,
        mut locations: Vec<Option<crate::backend::search::Location>>,
    ) {
        let (list, list_scroll, list_stack) = if threads {
            (&self.thread_list, &self.thread_scroll, &self.thread_stack)
        } else {
            (&self.list, &self.list_scroll, &self.list_stack)
        };
        if self.search_running[usize::from(threads)].get()
            && self.search_preserve_results[usize::from(threads)].get()
        {
            let present: HashSet<_> = groups
                .iter()
                .enumerate()
                .map(|(index, group)| {
                    if threads {
                        thread_key(group)
                    } else {
                        let sender = models::senders::key(&group[0]);
                        locations
                            .get(index)
                            .and_then(Option::as_ref)
                            .map_or(sender.clone(), |location| location.key(&sender))
                    }
                })
                .collect();
            let selected = self.search_filters.selected_accounts();
            let selected_folder = self.search_filters.folder();
            let old_locations = self.search_locations.borrow();
            for position in 0..ui::virtual_list::model(list).n_items() {
                ui::virtual_list::with_item(list, position, |row| {
                    if !present.contains(&row.key)
                        && (threads
                            || old_locations.get(&row.key).is_some_and(|location| {
                                selected.contains(&location.account)
                                    && selected_folder
                                        .as_ref()
                                        .is_none_or(|folder| &location.folder == folder)
                            }))
                    {
                        groups.push(row.messages.clone());
                        locations.push(old_locations.get(&row.key).cloned());
                    }
                });
            }
        }
        self.search_published_queries.borrow_mut()[usize::from(threads)] = query.to_owned();
        let selected_group = ui::virtual_list::selected(list).and_then(|index| {
            ui::virtual_list::with_item(list, index, |row| {
                (
                    row.key.clone(),
                    if threads {
                        row.messages
                            .iter()
                            .map(|message| (message.uid, message.message_id.clone()))
                            .collect::<HashSet<_>>()
                    } else {
                        HashSet::new()
                    },
                )
            })
        });
        let sender = if threads {
            self.sender_pane.borrow().sender().map(str::to_owned)
        } else {
            None
        };
        // Keep the active unread conversation in its unread sort position until
        // another conversation opens, while still displaying its real read state.
        let deferred = self.deferred_read_sort.borrow();
        let unread_first = if threads {
            self.thread_unread_first.unread_first()
        } else {
            self.sender_unread_first.unread_first()
        };
        let searching = if threads {
            !query.is_empty() || self.search_sender.borrow().is_some()
        } else {
            self.sender_search_active()
        };
        if !searching || (threads && query.is_empty()) {
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
        }
        if !threads {
            self.has_more_senders
                .set(!searching && groups.len() > self.visible_limit.get());
            if !searching {
                groups.truncate(self.visible_limit.get());
            }
        }
        if !threads
            && searching
            && let Some(order) = self.search_account_order.borrow().as_ref()
        {
            let positions: HashMap<_, _> = order
                .iter()
                .enumerate()
                .map(|(position, key)| (key.as_str(), position))
                .collect();
            let mut located: Vec<_> = groups.into_iter().zip(locations).collect();
            located.sort_by_key(|(group, location)| {
                let sender = models::senders::key(&group[0]);
                let key = location
                    .as_ref()
                    .map_or(sender.clone(), |location| location.key(&sender));
                positions.get(key.as_str()).copied().unwrap_or(usize::MAX)
            });
            (groups, locations) = located.into_iter().unzip();
        }
        let keys: Vec<String> = groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                if sender.is_some() {
                    thread_key(group)
                } else {
                    let sender = models::senders::key(&group[0]);
                    locations
                        .get(index)
                        .and_then(Option::as_ref)
                        .map_or(sender.clone(), |location| location.key(&sender))
                }
            })
            .collect();
        if !threads {
            if self.search_account_order.borrow().is_some() {
                let mut order = self.search_account_order.borrow_mut();
                let order = order.as_mut().unwrap();
                let known: HashSet<_> = order.iter().cloned().collect();
                order.extend(keys.iter().filter(|key| !known.contains(*key)).cloned());
            }
            *self.search_locations.borrow_mut() = keys
                .iter()
                .zip(&locations)
                .filter_map(|(key, location)| {
                    location.clone().map(|location| (key.clone(), location))
                })
                .collect();
        }
        let selected = selected_group.as_ref().and_then(|(key, identities)| {
            groups.iter().enumerate().position(|(position, group)| {
                if threads {
                    group.iter().any(|message| {
                        identities.contains(&(message.uid, message.message_id.clone()))
                    })
                } else {
                    keys[position] == *key
                }
            })
        });
        let caption_changed = !threads
            && self
                .search_multiaccount
                .replace(self.search_filters.selected_accounts().len() > 1)
                != (self.search_filters.selected_accounts().len() > 1);
        let changed: Vec<usize> = {
            let by_key: HashMap<String, u32> = (0..ui::virtual_list::model(list).n_items())
                .filter_map(|position| {
                    ui::virtual_list::with_item(list, position, |row| (row.key.clone(), position))
                })
                .collect();
            groups
                .iter()
                .enumerate()
                .filter_map(|(index, group)| {
                    by_key
                        .get(&keys[index])
                        .is_some_and(|&position| {
                            ui::virtual_list::with_item(list, position, |row| {
                                caption_changed || !same_row(&row.messages, group, threads)
                            })
                            .unwrap_or(false)
                        })
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
            ui::virtual_list::update_owned(list, rows, &changed, selected);
        });
        if !threads && searching {
            self.refresh_search_sender();
        }
        self.refresh_open_styles();
        if count == 0 {
            let loading = if searching {
                self.search_running[usize::from(threads)].get()
                    && self.search_show_progress[usize::from(threads)].get()
            } else if threads {
                self.sender_pane.borrow().loading()
            } else {
                self.loading.get() && !self.has_cached_folder()
            };
            if !threads && !loading && searching {
                ui::states::no_senders_found(list_stack);
                return;
            }
            ui::states::list_state(
                list_stack,
                if self.search_running[usize::from(threads)].get()
                    && self.search_show_progress[usize::from(threads)].get()
                {
                    "Searching…"
                } else if loading {
                    "Loading messages..."
                } else if searching {
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

    /// A body or read-state update affects existing rows, not their membership.
    /// Replace only changed rows instead of copying and regrouping the mailbox.
    fn refresh_message_rows(&self, message: &Message) {
        for threads in [false, true] {
            let list = if threads {
                &self.thread_list
            } else {
                &self.list
            };
            let model = ui::virtual_list::model(list);
            let replacement = self.visible_message(&message.list_header(threads));
            for position in 0..model.n_items() {
                let object = model
                    .item(position)
                    .unwrap()
                    .downcast::<glib::BoxedAnyObject>()
                    .unwrap();
                let mut row = object.borrow_mut::<ui::virtual_list::Row>();
                if !threads
                    && let Some(location) = self.search_locations.borrow().get(&row.key)
                    && (self
                        .account
                        .borrow()
                        .as_ref()
                        .is_none_or(|account| account.email != location.account)
                        || *self.folder.borrow() != location.folder)
                {
                    continue;
                }
                let Some(index) = row
                    .messages
                    .iter()
                    .position(|old| old.uid == message.uid && old.message_id == message.message_id)
                else {
                    continue;
                };
                let Some(replacement) = &replacement else {
                    continue;
                };
                let old = row.messages[index].list_header(threads);
                let changed = !same_row(
                    std::slice::from_ref(&old),
                    std::slice::from_ref(replacement),
                    threads,
                );
                row.messages[index] = replacement.clone();
                row.messages[index].search_match = old.search_match;
                let updated = changed.then(|| glib::BoxedAnyObject::new(row.clone()));
                drop(row);
                if let Some(updated) = updated {
                    model.splice(position, 1, std::slice::from_ref(&updated));
                }
            }
        }
    }

    pub(super) fn update_body(self: &Rc<Self>, message: &Message) {
        let mut message = message.clone();
        self.merge_read_states(std::slice::from_mut(&mut message));
        message.prepare_display();
        let search_changed = self
            .messages
            .borrow()
            .iter()
            .chain(self.sender_pane.borrow().messages().iter())
            .find(|old| old.uid == message.uid && old.message_id == message.message_id)
            .is_none_or(|old| {
                old.subject != message.subject
                    || old.sender != message.sender
                    || old.reply_to != message.reply_to
                    || old.recipients != message.recipients
                    || old.cc != message.cc
                    || old.body_text != message.body_text
                    || old.body_html != message.body_html
            });
        let calendar_changed = self
            .messages
            .borrow()
            .iter()
            .chain(self.sender_pane.borrow().messages().iter())
            .find(|old| old.uid == message.uid && old.message_id == message.message_id)
            .is_some_and(|old| old.calendar_events != message.calendar_events);
        let tickets_changed = self
            .messages
            .borrow()
            .iter()
            .chain(self.sender_pane.borrow().messages().iter())
            .find(|old| old.uid == message.uid && old.message_id == message.message_id)
            .is_none_or(|old| old.tickets != message.tickets);
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
        if let Some(search) = self.search_sender.borrow_mut().as_mut()
            && let Some(existing) = search
                .messages
                .iter_mut()
                .find(|old| old.uid == message.uid && old.message_id == message.message_id)
        {
            *existing = message.search_document();
            existing.search_match = true;
        }
        let Some(visible) = self.visible_message(&message) else {
            return;
        };
        let message = &visible;
        self.refresh_message_rows(message);
        *self.search_snapshots.borrow_mut() = [None, None];
        if search_changed {
            for threads in [false, true] {
                let entry = if threads {
                    &self.thread_search
                } else {
                    &self.search
                };
                if self.search_open(threads) && !entry.text().trim().is_empty() {
                    self.render_or_queue_search(threads);
                }
            }
        }
        if let Some(card) = self.cards.borrow().get(&message.uid) {
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                card.update(message)
            });
        }
        if has_new_parcel
            || calendar_changed
            || tickets_changed
            || message
                .calendar_events
                .iter()
                .any(|event| event.method.as_deref() == Some("REPLY"))
        {
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
    if unread(left) != unread(right)
        || (threads
            && left.iter().any(|message| message.is_spam)
                != right.iter().any(|message| message.is_spam))
        || left.iter().any(|message| message.is_flagged)
            != right.iter().any(|message| message.is_flagged)
    {
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
                    && a.calendar_events == b.calendar_events
                    && a.timestamp == b.timestamp
            }
            (None, None) => true,
            _ => false,
        };
        left.len() == right.len()
            && same_message
            && left
                .iter()
                .find(|message| !models::calendar::is_reply(message))
                .map(|message| (&message.body_text, &message.body_html))
                == right
                    .iter()
                    .find(|message| !models::calendar::is_reply(message))
                    .map(|message| (&message.body_text, &message.body_html))
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
        if left.iter().filter(|message| message.search_match).count()
            != right.iter().filter(|message| message.search_match).count()
        {
            return false;
        }
        match (
            models::senders::preview(left),
            models::senders::preview(right),
        ) {
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
    fn reply_classification_and_older_preview_updates_refresh_the_thread_row() {
        let latest = Message {
            uid: 2,
            body_text: "Accepted: Lunch".into(),
            ..Default::default()
        };
        let reply = Message {
            calendar_events: vec![models::calendar::Event {
                method: Some("REPLY".into()),
                ..Default::default()
            }],
            ..latest.clone()
        };
        let original = Message {
            uid: 1,
            body_text: "Join us".into(),
            ..Default::default()
        };
        assert!(!same_row(
            &[latest, original.clone()],
            &[reply.clone(), original.clone()],
            true
        ));
        assert!(!same_row(
            &[reply.clone(), original.clone()],
            &[
                reply,
                Message {
                    body_text: "Join us for lunch".into(),
                    ..original
                }
            ],
            true
        ));
    }

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
    fn additional_matches_refresh_the_sender_count_without_changing_the_preview() {
        let latest = Message {
            uid: 2,
            timestamp: 2,
            sender: "sender@example.com".into(),
            subject: "First match".into(),
            search_match: true,
            is_read: true,
            ..Default::default()
        };
        let older = Message {
            uid: 1,
            timestamp: 1,
            subject: "Second match".into(),
            ..latest.clone()
        };
        assert!(!same_row(
            std::slice::from_ref(&latest),
            &[latest.clone(), older],
            false,
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
            std::slice::from_ref(&latest),
            &[latest.clone(), older.clone()],
            false
        ));
        assert!(!same_row(
            std::slice::from_ref(&latest),
            &[latest.clone(), older.clone()],
            true
        ));
        let unread_latest = Message {
            is_read: false,
            ..latest.clone()
        };
        let unread_older = Message {
            is_read: false,
            ..older.clone()
        };
        assert!(!same_row(
            &[unread_latest, unread_older.clone()],
            &[latest.clone(), unread_older.clone()],
            false
        ));
        let edited = Message {
            subject: "Updated unread subject".into(),
            ..unread_older.clone()
        };
        assert!(!same_row(
            &[latest.clone(), unread_older.clone()],
            &[latest.clone(), edited],
            false
        ));
        assert!(!same_row(
            &[latest.clone(), unread_older],
            &[latest, older],
            false
        ));
    }
}
