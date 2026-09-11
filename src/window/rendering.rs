use super::*;

impl State {
    pub(super) fn render_list(&self) {
        self.rendering.set(true);
        let old = self.groups.borrow().clone();
        let sender = self.selected_sender.borrow().clone();
        let query = self.search.text().trim().to_lowercase();
        let groups = if let Some(sender) = &sender {
            let messages: Vec<_> = self
                .messages
                .borrow()
                .iter()
                .filter(|message| models::senders::key(message) == *sender)
                .cloned()
                .collect();
            models::threads(&messages, &query)
        } else {
            models::senders::groups(&self.messages.borrow(), &query)
        };
        let related = |left: &[Message], right: &[Message]| {
            if sender.is_some() {
                overlaps(left, right)
            } else {
                models::senders::key(&left[0]) == models::senders::key(&right[0])
            }
        };
        let selected_row = self.list.selected_row();
        let rows: Vec<_> = (0..old.len())
            .filter_map(|i| self.list.row_at_index(i as i32))
            .collect();
        let mut used = HashSet::new();
        ui::scroll_position::preserve(&self.list_scroll, &self.list, || {
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
                if self.list.row_at_index(index as i32).as_ref() != Some(&row) {
                    if row.parent().is_some() {
                        self.list.unselect_row(&row);
                        self.list.remove(&row);
                    }
                    self.list.insert(&row, index as i32);
                }
            }
            for (index, row) in rows.iter().enumerate() {
                if !used.contains(&index) && row.parent().is_some() {
                    self.list.remove(row);
                }
            }
            if let Some(row) = selected_row
                && row.parent().is_some()
                && self.list.selected_row().as_ref() != Some(&row)
            {
                self.list.select_row(Some(&row));
            }
        });
        if groups.is_empty() {
            ui::states::list_state(
                &self.list_stack,
                if self.loading.get() {
                    "Loading messages..."
                } else if !query.is_empty() {
                    "No matching messages"
                } else if sender.is_some() {
                    "No messages from this sender"
                } else {
                    "No messages in this folder"
                },
                self.loading.get(),
                false,
            );
        } else {
            self.list_stack.set_visible_child_name("list");
        }
        *self.groups.borrow_mut() = groups;
        self.rendering.set(false);
        self.load_previews();
    }

    pub(super) fn update_body(&self, message: &Message) {
        let mut read_changed = false;
        if let Some(existing) = self
            .messages
            .borrow_mut()
            .iter_mut()
            .find(|m| m.uid == message.uid)
        {
            read_changed = existing.is_read != message.is_read;
            *existing = message.clone();
        }
        let mut groups = self.groups.borrow_mut();
        for (index, group) in groups.iter_mut().enumerate() {
            if let Some(existing) = group.iter_mut().find(|m| m.uid == message.uid) {
                let flags_changed = existing.is_read != message.is_read
                    || existing.is_flagged != message.is_flagged;
                let body_changed = existing.body_text != message.body_text
                    || existing.body_html != message.body_html;
                *existing = message.clone();
                if (flags_changed || body_changed)
                    && let Some(row) = self.list.row_at_index(index as i32)
                {
                    if self.selected_sender.borrow().is_some() {
                        ui::update_message_row(&row, group);
                    } else {
                        ui::update_sender(&row, group, &self.avatars);
                    }
                }
                break;
            }
        }
        drop(groups);
        if read_changed {
            self.render_list();
        }
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
