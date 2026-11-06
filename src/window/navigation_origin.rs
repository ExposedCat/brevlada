use super::*;

pub(super) struct Origin {
    order: Vec<String>,
    position: usize,
    messages: HashSet<(u32, String)>,
}

impl Origin {
    pub fn capture(list: &gtk::ListView, position: u32) -> Option<Self> {
        let messages = ui::virtual_list::with_item(list, position, |row| {
            row.messages
                .iter()
                .map(|message| (message.uid, message.message_id.clone()))
                .collect()
        })?;
        let order = (0..ui::virtual_list::model(list).n_items())
            .filter_map(|position| {
                ui::virtual_list::with_item(list, position, |row| row.key.clone())
            })
            .collect();
        Some(Self {
            order,
            position: position as usize,
            messages,
        })
    }

    pub fn position(&self) -> u32 {
        self.position as u32
    }

    pub fn next(
        &self,
        list: &gtk::ListView,
        keep_current: bool,
        eligible: impl Fn(&ui::virtual_list::Row) -> bool,
    ) -> Option<u32> {
        let positions: HashMap<_, _> = (0..ui::virtual_list::model(list).n_items())
            .filter_map(|position| {
                ui::virtual_list::with_item(list, position, |row| {
                    eligible(row).then(|| (row.key.clone(), position))
                })
                .flatten()
            })
            .collect();
        if keep_current {
            if let Some(position) = positions.get(&self.order[self.position]) {
                return Some(*position);
            }
            if let Some(position) = (0..ui::virtual_list::model(list).n_items()).find(|&position| {
                ui::virtual_list::with_item(list, position, |row| {
                    eligible(row)
                        && row.messages.iter().any(|message| {
                            self.messages
                                .contains(&(message.uid, message.message_id.clone()))
                        })
                })
                .unwrap_or(false)
            }) {
                return Some(position);
            }
        }
        self.order[self.position + 1..]
            .iter()
            .chain(self.order[..self.position].iter().rev())
            .find_map(|key| positions.get(key).copied())
    }
}

impl State {
    pub(super) fn remember_sender_origin(&self, sender: &str) {
        let located = self.account.borrow().as_ref().map(|account| {
            crate::backend::search::Location {
                account: account.email.clone(),
                folder: self.folder.borrow().clone(),
            }
            .key(sender)
        });
        self.navigation_origins.borrow_mut()[0] = (0..ui::virtual_list::model(&self.list)
            .n_items())
            .find(|&position| {
                ui::virtual_list::with_item(&self.list, position, |row| {
                    row.key == sender || located.as_ref() == Some(&row.key)
                })
                .unwrap_or(false)
            })
            .and_then(|position| Origin::capture(&self.list, position));
    }

    pub(super) fn remember_thread_origin(&self, group: &[Message]) {
        self.navigation_origins.borrow_mut()[1] = (0..ui::virtual_list::model(&self.thread_list)
            .n_items())
            .find(|&position| {
                ui::virtual_list::with_item(&self.thread_list, position, |row| {
                    super::rendering::overlaps(&row.messages, group)
                })
                .unwrap_or(false)
            })
            .and_then(|position| Origin::capture(&self.thread_list, position));
    }
}
