use super::*;

impl State {
    pub(super) fn connect_lists(self: &Rc<Self>) {
        let state = self;
        for threads in [false, true] {
            let list = if threads {
                &state.thread_list
            } else {
                &state.list
            };
            let factory = gtk::SignalListItemFactory::new();
            let weak = Rc::downgrade(state);
            factory.connect_bind(move |_, object| {
                let Some(state) = weak.upgrade() else { return };
                let item = object
                    .downcast_ref::<gtk::ListItem>()
                    .expect("List view factory receives a list item");
                let Some(object) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                    return;
                };
                let data = object.borrow::<ui::virtual_list::Row>();
                let group = &data.messages;
                let row = if threads {
                    ui::message_row(group)
                } else {
                    ui::sender_row(group, &state.avatars)
                };
                if !threads && let Some(location) = state.search_locations.borrow().get(&data.key) {
                    row.set_tooltip_text(Some(&format!(
                        "{} · {}",
                        location.account, location.folder
                    )));
                    if let Some(content) = row
                        .first_child()
                        .and_then(|avatar| avatar.next_sibling())
                        .and_downcast::<gtk::Box>()
                    {
                        let caption = if state.search_filters.selected_accounts().len() > 1 {
                            format!("{} · {}", location.account, location.folder)
                        } else {
                            location.folder.clone()
                        };
                        let account = ui::label(&caption, "dim-label");
                        account.add_css_class("caption");
                        content.append(&account);
                    }
                }
                let active = if threads {
                    &state.active_thread_row
                } else {
                    &state.active_sender_row
                };
                active.bind(&row, data.key.clone());
                if threads
                    && let Some(message) = group
                        .iter()
                        .find(|message| !models::calendar::is_reply(message))
                {
                    state.request_preview(message);
                }
                ui::virtual_list::identify(&row, &data.key);
                item.set_child(Some(&row));
            });
            factory.connect_unbind(|_, object| {
                if let Some(item) = object.downcast_ref::<gtk::ListItem>() {
                    item.set_child(None::<&gtk::Widget>);
                }
            });
            list.set_factory(Some(&factory));
        }
        let weak = Rc::downgrade(state);
        state.list.connect_activate(move |_, position| {
            if let Some(state) = weak.upgrade() {
                state.activate_sender(position);
            }
        });
        let weak = Rc::downgrade(state);
        state.thread_list.connect_activate(move |_, position| {
            if let Some(state) = weak.upgrade() {
                if state.rendering.get() {
                    return;
                }
                let group =
                    ui::virtual_list::item(&state.thread_list, position).map(|row| row.messages);
                if let Some(group) = group {
                    state.show_thread(group);
                }
            }
        });
    }

    pub(super) fn activate_sender(self: &Rc<Self>, position: u32) {
        if self.rendering.get() {
            return;
        }
        let Some((key, location, message, matches)) =
            ui::virtual_list::with_item(&self.list, position, |row| {
                let message = models::senders::preview(&row.messages).cloned();
                let matches = message
                    .as_ref()
                    .is_some_and(|message| message.search_match)
                    .then(|| row.messages.clone());
                (
                    row.key.clone(),
                    self.search_locations.borrow().get(&row.key).cloned(),
                    message,
                    matches,
                )
            })
        else {
            return;
        };
        if !self.select_search_location(location.as_ref()) {
            return;
        }
        if let Some(matches) = matches {
            self.open_search_sender(key, matches);
        } else if let Some(message) = message {
            self.filter_sender(Some(message));
        }
    }
}
