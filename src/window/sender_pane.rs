use super::*;

impl State {
    pub(super) fn reset_threads(&self) {
        self.rendering.set(true);
        ui::virtual_list::replace(&self.thread_list, 0, None);
        self.rendering.set(false);
    }

    pub(super) fn filter_sender(&self, message: Option<Message>) {
        self.next_message.set(None);
        let sender = message.as_ref().map(models::senders::key);
        let opening = sender.is_some() && self.sender_pane.borrow().sender().is_none();
        if !self.sender_pane.borrow_mut().select(sender) {
            return;
        }
        self.reset_threads();
        self.thread_load_more.set_visible(false);
        if opening {
            let was_expanded = self.account_sidebar.get_visible();
            self.account_sidebar.set_visible(false);
            self.restore_accounts_on_back.set(was_expanded);
        } else if message.is_none() && self.restore_accounts_on_back.replace(false) {
            self.account_sidebar.set_visible(true);
        }
        self.thread_sidebar.set_visible(message.is_some());
        self.back.set_visible(message.is_some());
        if message.is_some() {
            self.load_sender_page();
            self.render_sender_pane();
        }
        self.refresh_open_styles();
        ui::virtual_list::selection(&self.list).set_selected(gtk::INVALID_LIST_POSITION);
        self.thread_scroll.vadjustment().set_value(0.0);
    }

    pub(super) fn load_sender_page(&self) {
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        let Some(page) = self.sender_pane.borrow_mut().request() else {
            return;
        };
        self.thread_load_more.set_sensitive(false);
        self.thread_load_more.set_visible(false);
        if let Err(error) = self.sender.send(Command::LoadSenderPage {
            account,
            folder: self.folder.borrow().clone(),
            sender: page.sender,
            generation: self.generation.get(),
            sender_generation: page.ticket,
            before: page.before,
            limit: 25,
        }) {
            self.sender_pane.borrow_mut().fail(page.ticket);
            self.thread_load_more.set_visible(true);
            self.thread_load_more.set_sensitive(true);
            self.toast.add_toast(adw::Toast::new(&error.to_string()));
        }
    }

    pub(super) fn maybe_load_sender_page(&self) {
        if self.rendering.get()
            || self.sender_pane.borrow().loading()
            || !self.sender_pane.borrow().can_load()
        {
            return;
        }
        let adjustment = self.thread_scroll.vadjustment();
        if adjustment.page_size() > 0.0
            && adjustment.upper() - adjustment.value() - adjustment.page_size() < 400.0
        {
            self.load_sender_page();
        }
    }
}
