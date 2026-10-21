use super::*;

pub(super) struct SenderMatches {
    pub key: String,
    pub messages: Vec<Message>,
}

impl State {
    pub(super) fn preserve_sender_matches(
        &self,
        groups: &mut [Vec<Message>],
        locations: &[Option<crate::backend::search::Location>],
    ) {
        let positions: HashMap<_, _> = (0..ui::virtual_list::model(&self.list).n_items())
            .filter_map(|position| {
                ui::virtual_list::with_item(&self.list, position, |row| (row.key.clone(), position))
            })
            .collect();
        for (index, group) in groups.iter_mut().enumerate() {
            let sender = models::senders::list_key(&group[0]);
            let key = locations
                .get(index)
                .and_then(Option::as_ref)
                .map_or(sender.clone(), |location| location.key(&sender));
            if let Some(position) = positions.get(&key).copied() {
                ui::virtual_list::with_item(&self.list, position, |row| {
                    let incoming: HashSet<_> = group
                        .iter()
                        .map(|message| {
                            (
                                message.uid,
                                message.message_id.clone(),
                                message.uid_validity,
                            )
                        })
                        .collect();
                    group.extend(
                        row.messages
                            .iter()
                            .filter(|message| {
                                !incoming.contains(&(
                                    message.uid,
                                    message.message_id.clone(),
                                    message.uid_validity,
                                ))
                            })
                            .cloned(),
                    );
                });
                group.sort_by_key(|message| std::cmp::Reverse((message.timestamp, message.uid)));
            }
        }
    }

    pub(super) fn open_search_sender(self: &Rc<Self>, key: String, messages: Vec<Message>) {
        let Some(mut message) = models::senders::preview(&messages).cloned() else {
            return;
        };
        let target = (message.uid, message.message_id.clone());
        message.search_match = false;
        self.filter_sender(Some(message.clone()));
        self.thread_search.set_text("");
        *self.search_sender.borrow_mut() = Some(SenderMatches { key, messages });
        self.reset_threads();
        self.render_sender_pane();
        self.show_thread(vec![message]);
        *self.search_message.borrow_mut() = Some(target);
        self.reveal_search_message();
    }

    pub(super) fn matched_sender_messages(&self) -> Option<Vec<Message>> {
        let search = self.search_sender.borrow();
        let search = search.as_ref()?;
        let mut messages: HashMap<_, _> = search
            .messages
            .iter()
            .map(|message| ((message.uid, message.message_id.clone()), message.clone()))
            .collect();
        for message in self
            .messages
            .borrow()
            .iter()
            .chain(self.sender_pane.borrow().messages())
            .chain(self.open_group.borrow().iter())
        {
            if let Some(matched) = messages.get_mut(&(message.uid, message.message_id.clone()))
                && (message.body_loaded || !matched.body_loaded)
            {
                *matched = message.clone();
            }
        }
        models::read_state::merge_into(
            messages.values_mut(),
            self.messages
                .borrow()
                .iter()
                .chain(self.sender_pane.borrow().messages())
                .chain(self.open_group.borrow().iter()),
        );
        Some(self.visible_group(&messages.into_values().collect::<Vec<_>>()))
    }

    pub(super) fn refresh_search_sender(&self) {
        let changed = {
            let mut search = self.search_sender.borrow_mut();
            search.as_mut().is_some_and(|search| {
                let mut matches = (0..ui::virtual_list::model(&self.list).n_items())
                    .find_map(|position| {
                        ui::virtual_list::with_item(&self.list, position, |row| {
                            (row.key == search.key).then(|| row.messages.clone())
                        })
                        .flatten()
                    })
                    .unwrap_or_default();
                for message in &mut matches {
                    if let Some(loaded) = search.messages.iter().find(|old| {
                        old.uid == message.uid
                            && old.message_id == message.message_id
                            && old.body_loaded
                    }) {
                        message.body_text = loaded.body_text.clone();
                        message.body_html = loaded.body_html.clone();
                        message.body_loaded = true;
                        message.list_preview = loaded.list_preview.clone();
                    }
                }
                if search.messages == matches {
                    return false;
                }
                search.messages = matches;
                true
            })
        };
        if changed {
            self.search_snapshots.borrow_mut()[1] = None;
            self.cancel_search(true);
            self.render_or_queue_search(true);
        }
    }
}
