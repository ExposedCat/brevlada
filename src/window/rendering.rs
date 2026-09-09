use super::*;

impl State {
    pub(super) fn render_list(&self) {
        self.rendering.set(true);
        let old = self.groups.borrow().clone();
        let mut groups = models::threads(
            &self.messages.borrow(),
            &self.search.text().trim().to_lowercase(),
        );
        groups.sort_by_key(|group| {
            old.iter()
                .position(|previous| overlaps(previous, group))
                .map(|i| i + 1)
                .unwrap_or(0)
        });
        let rows: Vec<_> = (0..old.len())
            .filter_map(|i| self.list.row_at_index(i as i32))
            .collect();
        let mut used = HashSet::new();
        ui::scroll_position::preserve(&self.list_scroll, &self.list, || {
            for (index, group) in groups.iter().enumerate() {
                let previous = old
                    .iter()
                    .enumerate()
                    .position(|(i, previous)| !used.contains(&i) && overlaps(previous, group));
                let row = if let Some(previous) = previous {
                    used.insert(previous);
                    let row = rows[previous].clone();
                    if old[previous] != *group {
                        ui::update_message_row(&row, group);
                    }
                    row
                } else {
                    ui::message_row(group)
                };
                if self.list.row_at_index(index as i32).as_ref() != Some(&row) {
                    if row.parent().is_some() {
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
        });
        if groups.is_empty() {
            ui::states::list_state(
                &self.list_stack,
                if self.loading.get() {
                    "Loading messages..."
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
        let mut groups = self.groups.borrow_mut();
        for (index, group) in groups.iter_mut().enumerate() {
            if let Some(existing) = group.iter_mut().find(|m| m.uid == message.uid) {
                let flags_changed = existing.is_read != message.is_read
                    || existing.is_flagged != message.is_flagged;
                *existing = message.clone();
                if flags_changed && let Some(row) = self.list.row_at_index(index as i32) {
                    ui::update_message_row(&row, group);
                }
                break;
            }
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
