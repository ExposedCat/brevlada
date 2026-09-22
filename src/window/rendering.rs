use super::*;

impl State {
    pub(super) fn render_list(&self) {
        self.rendering.set(true);
        self.render_pane(false);
        if self.selected_sender.borrow().is_some() {
            self.render_pane(true);
        }
        self.rendering.set(false);
        self.load_previews();
    }

    fn render_pane(&self, threads: bool) {
        let (list, list_scroll, list_stack, stored_groups) = if threads {
            (
                &self.thread_list,
                &self.thread_scroll,
                &self.thread_stack,
                &self.thread_groups,
            )
        } else {
            (
                &self.list,
                &self.list_scroll,
                &self.list_stack,
                &self.groups,
            )
        };
        let old = stored_groups.borrow().clone();
        let sender = if threads {
            self.selected_sender.borrow().clone()
        } else {
            None
        };
        let query = self.search.text().trim().to_lowercase();
        let messages = self.visible_messages();
        let mut groups = if let Some(sender) = &sender {
            let messages: Vec<_> = messages
                .into_iter()
                .filter(|message| models::senders::key(message) == *sender)
                .collect();
            models::threads(&messages, &query)
        } else {
            models::senders::groups(&messages, &query)
        };
        // Keep the active unread conversation in its unread sort position until
        // another conversation opens, while still displaying its real read state.
        let deferred = self.deferred_read_sort.borrow();
        groups.sort_by_key(|group| {
            std::cmp::Reverse((
                group
                    .iter()
                    .any(|message| !message.is_read || deferred.contains(&message.uid)),
                group[0].timestamp,
                group[0].uid,
            ))
        });
        let related = |left: &[Message], right: &[Message]| {
            if sender.is_some() {
                overlaps(left, right)
            } else {
                models::senders::key(&left[0]) == models::senders::key(&right[0])
            }
        };
        let selected_row = list.selected_row();
        let rows: Vec<_> = (0..old.len())
            .filter_map(|i| list.row_at_index(i as i32))
            .collect();
        let mut used = HashSet::new();
        ui::scroll_position::preserve_offset(list_scroll, || {
            for (index, group) in groups.iter().enumerate() {
                let previous = old
                    .iter()
                    .enumerate()
                    .position(|(i, previous)| !used.contains(&i) && related(previous, group));
                let row = if let Some(previous) = previous {
                    used.insert(previous);
                    let row = rows[previous].clone();
                    if old[previous] != *group {
                        if sender.is_some() {
                            ui::update_message_row(&row, group);
                        } else {
                            ui::update_sender(&row, group, &self.avatars);
                        }
                    }
                    row
                } else if sender.is_some() {
                    ui::message_row(group)
                } else {
                    ui::sender_row(group, &self.avatars)
                };
                if list.row_at_index(index as i32).as_ref() != Some(&row) {
                    if row.parent().is_some() {
                        list.unselect_row(&row);
                        list.remove(&row);
                    }
                    list.insert(&row, index as i32);
                }
            }
            for (index, row) in rows.iter().enumerate() {
                if !used.contains(&index) && row.parent().is_some() {
                    list.remove(row);
                }
            }
            if let Some(row) = selected_row
                && row.parent().is_some()
                && list.selected_row().as_ref() != Some(&row)
            {
                list.select_row(Some(&row));
            }
        });
        if groups.is_empty() {
            let loading = self.loading.get() && !self.has_cached_folder();
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
        *stored_groups.borrow_mut() = groups;
    }

    pub(super) fn update_body(&self, message: &Message) {
        if let Some(existing) = self
            .messages
            .borrow_mut()
            .iter_mut()
            .find(|m| m.uid == message.uid)
        {
            *existing = message.clone();
        }
        let Some(visible) = self.visible_message(message) else {
            return;
        };
        let message = &visible;
        self.render_list();
        if let Some(card) = self.cards.borrow().get(&message.uid) {
            ui::scroll_position::preserve(&self.viewer_scroll, &self.viewer, || {
                card.update(message)
            });
        }
    }
}

fn overlaps(left: &[Message], right: &[Message]) -> bool {
    left.iter().any(|a| right.iter().any(|b| a.uid == b.uid))
}
