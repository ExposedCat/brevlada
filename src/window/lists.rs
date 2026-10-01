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
                let active = if threads {
                    &state.active_thread_row
                } else {
                    &state.active_sender_row
                };
                active.bind(&row, data.key.clone());
                if threads && let Some(message) = group.first() {
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
                if state.rendering.get() {
                    return;
                }
                let message = ui::virtual_list::item(&state.list, position)
                    .and_then(|row| row.messages.first().cloned());
                if let Some(message) = message {
                    state.filter_sender(Some(message));
                }
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
}
