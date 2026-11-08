use super::*;

pub(super) struct Row {
    item: glib::WeakRef<gtk::ListItem>,
    screen: Weak<Subscriptions>,
    check: gtk::CheckButton,
    logo: gtk::Box,
    name: gtk::Label,
    email: gtk::Label,
    account: gtk::Label,
    count: gtk::Label,
    unsubscribe: gtk::Button,
    bound_sender: RefCell<String>,
}

impl Row {
    pub fn attach(item: &gtk::ListItem, screen: &Rc<Subscriptions>) {
        item.set_selectable(false);
        let widget = adw::PreferencesRow::builder()
            .use_markup(false)
            .activatable(false)
            .selectable(false)
            .focusable(false)
            .css_classes(["subscriptions-row"])
            .build();
        let grid = gtk::Grid::builder()
            .column_homogeneous(true)
            .column_spacing(theme::SPACING)
            .hexpand(true)
            .build();
        let sender = super::super::horizontal("subscriptions-sender", theme::SPACING);
        let check = gtk::CheckButton::new();
        check.set_valign(gtk::Align::Center);
        let logo = super::super::column("subscriptions-logo");
        logo.set_valign(gtk::Align::Center);
        let name = super::super::label("", "");
        name.set_valign(gtk::Align::Center);
        name.set_halign(gtk::Align::Fill);
        sender.append(&check);
        sender.append(&logo);
        sender.append(&name);
        let identity = super::super::column("subscriptions-identity");
        identity.set_valign(gtk::Align::Center);
        let email = super::super::label("", "");
        email.set_max_width_chars(theme::SUBSCRIPTIONS_EMAIL_CHARS);
        let account = super::super::label("", "caption");
        account.set_max_width_chars(theme::SUBSCRIPTIONS_EMAIL_CHARS);
        account.add_css_class("dim-label");
        for label in [&email, &account] {
            label.set_halign(gtk::Align::Fill);
            label.set_xalign(0.5);
        }
        identity.append(&email);
        identity.append(&account);
        let count = super::super::label("", "dim-label");
        count.set_hexpand(true);
        count.set_halign(gtk::Align::Fill);
        count.set_valign(gtk::Align::Center);
        count.set_xalign(0.5);
        let unsubscribe = gtk::Button::with_label("Unsubscribe");
        unsubscribe.set_valign(gtk::Align::Center);
        unsubscribe.set_halign(gtk::Align::Center);
        unsubscribe.set_hexpand(true);
        grid.attach(&sender, 0, 0, 1, 1);
        grid.attach(&identity, 1, 0, 1, 1);
        grid.attach(&count, 2, 0, 1, 1);
        grid.attach(&unsubscribe, 3, 0, 1, 1);
        widget.set_child(Some(&grid));
        item.set_child(Some(&widget));
        let row = Rc::new(Self {
            item: item.downgrade(),
            screen: Rc::downgrade(screen),
            check,
            logo,
            name,
            email,
            account,
            count,
            unsubscribe,
            bound_sender: RefCell::default(),
        });
        screen.rows.borrow_mut().push(Rc::downgrade(&row));
        let target = row.clone();
        item.connect_item_notify(move |_| target.refresh());
        let target = row.clone();
        item.connect_selected_notify(move |_| target.refresh_selection());
        let weak = Rc::downgrade(&row);
        row.check.connect_toggled(move |check| {
            if let Some(row) = weak.upgrade()
                && let Some(screen) = row.screen.upgrade()
                && let Some(item) = row.item.upgrade()
                && !screen.blocked.get()
                && item.position() != gtk::INVALID_LIST_POSITION
                && screen.selection.is_selected(item.position()) != check.is_active()
            {
                if check.is_active() {
                    screen.selection.select_item(item.position(), false);
                } else {
                    screen.selection.unselect_item(item.position());
                }
            }
        });
        let weak = Rc::downgrade(&row);
        row.unsubscribe.connect_clicked(move |_| {
            if let Some(row) = weak.upgrade()
                && let Some(screen) = row.screen.upgrade()
                && let Some(entry) = row.entry()
                && !screen.blocked.get()
            {
                let status = screen.statuses.borrow().get(&entry.id).cloned();
                if let Some(Status::Manual(url)) = status {
                    screen.open(&url);
                } else if !matches!(status, Some(Status::Done | Status::Pending)) {
                    screen.request(vec![entry], matches!(status, Some(Status::Failed(_))));
                }
            }
        });
    }

    fn entry(&self) -> Option<Entry> {
        Some(
            self.item
                .upgrade()?
                .item()?
                .downcast::<glib::BoxedAnyObject>()
                .ok()?
                .borrow::<Entry>()
                .clone(),
        )
    }

    fn refresh_selection(&self) {
        if let Some(item) = self.item.upgrade() {
            self.check.set_active(item.is_selected());
        }
    }

    pub fn refresh(&self) {
        let Some(screen) = self.screen.upgrade() else {
            return;
        };
        let Some(entry) = self.entry() else { return };
        let (name, email) = senders::identity(&entry.source.message);
        let name = if name.is_empty() { &email } else { &name };
        self.name.set_text(name);
        self.name.set_tooltip_text(Some(name));
        self.email.set_text(&email);
        self.email.set_tooltip_text(Some(&email));
        self.account.set_text(&entry.source.account);
        self.account.set_tooltip_text(Some(&entry.source.account));
        self.count
            .set_text(&format!("{} in last 30 days", entry.recent_emails));
        self.count.set_tooltip_text(Some(
            "Cached emails; copies in multiple folders count once.",
        ));
        let sender = senders::key(&entry.source.message);
        if *self.bound_sender.borrow() != sender {
            super::super::clear(&self.logo);
            let avatar = adw::Avatar::new(theme::SENDER_AVATAR_SIZE, Some(name), true);
            screen.avatars.attach(&avatar, &sender);
            self.logo.append(&avatar);
            *self.bound_sender.borrow_mut() = sender;
        }
        let status = screen.statuses.borrow().get(&entry.id).cloned();
        if matches!(status, Some(Status::Failed(_))) {
            self.name.add_css_class("error");
        } else {
            self.name.remove_css_class("error");
        }
        self.unsubscribe.set_label(match &status {
            Some(Status::Pending) => "Unsubscribing…",
            Some(Status::Done) => "Unsubscribed",
            Some(Status::Manual(_)) => "Open",
            Some(Status::Failed(_)) => "Retry",
            None => "Unsubscribe",
        });
        self.unsubscribe.set_tooltip_text(match &status {
            Some(Status::Failed(error)) => Some(error),
            Some(Status::Manual(_)) => Some("Complete unsubscription on the sender’s website"),
            _ => None,
        });
        let unavailable =
            screen.blocked.get() || matches!(status, Some(Status::Done | Status::Pending));
        self.unsubscribe.set_sensitive(!unavailable);
        self.check.set_sensitive(!unavailable);
        self.check
            .update_property(&[gtk::accessible::Property::Label(&format!("Select {name}"))]);
        self.refresh_selection();
    }
}
