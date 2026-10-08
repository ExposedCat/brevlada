use super::*;
use models::settings::Settings;
use std::path::PathBuf;

impl State {
    pub(super) fn connect_settings(
        self: &Rc<Self>,
        window: &adw::ApplicationWindow,
        action: &gtk::gio::SimpleAction,
        path: PathBuf,
    ) {
        *self.settings.borrow_mut() = Settings::load(&path).unwrap_or_else(|error| {
            eprintln!("Could not restore settings: {error}");
            Settings::default()
        });
        let parent = window.downgrade();
        let preferences = glib::WeakRef::<adw::PreferencesWindow>::new();
        let weak = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            if let Some(window) = preferences.upgrade() {
                window.present();
                return;
            }
            let Some(state) = weak.upgrade() else { return };
            let Some(parent) = parent.upgrade() else {
                return;
            };
            let settings = ui::settings::SettingsWindow::new(
                &parent,
                state.settings.borrow().trust_all_senders,
            );
            let weak = Rc::downgrade(&state);
            let path = path.clone();
            settings
                .trust_all_senders
                .connect_active_notify(move |row| {
                    if let Some(state) = weak.upgrade() {
                        state.settings.borrow_mut().trust_all_senders = row.is_active();
                        if let Err(error) = state.settings.borrow().save(&path) {
                            eprintln!("Could not save settings: {error}");
                            state
                                .toast
                                .add_toast(adw::Toast::new("Could not save settings"));
                        }
                        state.refresh_trust();
                    }
                });
            preferences.set(Some(&settings.window));
            settings.window.present();
        });
    }

    fn refresh_trust(&self) {
        let own = self
            .account
            .borrow()
            .as_ref()
            .map(|account| models::senders::address(&account.email));
        let cards: Vec<_> = self
            .open_group
            .borrow()
            .iter()
            .filter_map(|message| {
                let sender = models::senders::key(message);
                let card = self.cards.borrow().get(&message.uid)?.clone();
                Some((
                    card,
                    own.as_deref() == Some(&sender) || self.is_trusted(&sender),
                ))
            })
            .chain(self.related_sent.borrow().iter().filter_map(|item| {
                let sender = models::senders::key(&item.message);
                let card = self
                    .sent_cards
                    .borrow()
                    .get(&item.message.message_id)?
                    .clone();
                Some((
                    card,
                    own.as_deref() == Some(&sender) || self.is_trusted(&sender),
                ))
            }))
            .collect();
        for (card, trusted) in cards {
            card.set_trusted(trusted);
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn preferences() -> adw::PreferencesWindow {
        gtk::Window::list_toplevels()
            .into_iter()
            .find_map(|window| window.downcast::<adw::PreferencesWindow>().ok())
            .unwrap()
    }

    fn descendant<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Option<T> {
        if let Ok(target) = widget.clone().downcast::<T>() {
            return Some(target);
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(target) = descendant(&widget) {
                return Some(target);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn open_privacy(window: &adw::PreferencesWindow) -> adw::SwitchRow {
        let overview = window.visible_page().unwrap();
        let section = descendant::<adw::ActionRow>(overview.upcast_ref()).unwrap();
        assert_eq!(section.title(), "Privacy");
        section.emit_by_name::<()>("activated", &[]);
        let row = descendant::<adw::SwitchRow>(window.upcast_ref()).unwrap();
        let page = row
            .ancestor(adw::NavigationPage::static_type())
            .and_downcast::<adw::NavigationPage>()
            .unwrap();
        assert_eq!(page.title(), "Privacy");
        let navigation = page
            .ancestor(adw::NavigationView::static_type())
            .and_downcast::<adw::NavigationView>()
            .unwrap();
        assert_eq!(navigation.visible_page().as_ref(), Some(&page));
        row
    }

    #[test]
    #[ignore = "Requires a graphical session; opens isolated settings without mail workers"]
    fn settings_menu_persists_global_trust_and_preserves_individual_choices() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSettingsDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = ui::shell::Shell::new(&app);
        let window = shell.window.clone();
        let action = shell.settings.clone();
        let (worker, _) = worker::Worker::recording();
        let state = State::new(
            shell,
            worker,
            ui::expansion::Expansion::default(),
            ui::avatars::Avatars::new(|_| {}),
        );
        let directory = std::env::temp_dir().join(format!(
            "brevlada-settings-ui-{}",
            glib::uuid_string_random()
        ));
        let path = directory.join("settings.json");
        state.connect_settings(&window, &action, path.clone());
        *state.account.borrow_mut() = Some(Account {
            email: "me@example.com".into(),
            path: String::new(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: true,
            tls: false,
            oauth2: false,
            smtp: None,
        });
        *state.trusted_senders.borrow_mut() = HashMap::from([(
            "me@example.com".into(),
            HashSet::from(["trusted@example.com".into()]),
        )]);
        assert!(!state.is_trusted("unknown@example.com"));
        assert!(state.is_trusted("trusted@example.com"));
        action.activate(None);
        let settings = preferences();
        assert!(descendant::<adw::SwitchRow>(settings.upcast_ref()).is_none());
        let row = open_privacy(&settings);
        assert_eq!(row.title(), "Trust all senders");
        assert!(!row.is_active());
        row.set_active(true);
        assert!(state.is_trusted("unknown@example.com"));
        assert!(Settings::load(&path).unwrap().trust_all_senders);
        assert!(!state.is_sender_trusted("unknown@example.com"));
        assert!(settings.pop_subpage());
        let row = open_privacy(&settings);
        assert!(row.is_active());
        action.activate(None);
        assert_eq!(preferences(), settings);
        settings.destroy();
        drop(row);
        drop(settings);
        action.activate(None);
        let settings = preferences();
        let row = open_privacy(&settings);
        assert!(row.is_active());
        row.set_active(false);
        assert!(!state.is_trusted("unknown@example.com"));
        assert!(state.is_trusted("trusted@example.com"));
        assert!(!Settings::load(&path).unwrap().trust_all_senders);
        settings.destroy();
        window.destroy();
        std::fs::remove_dir_all(directory).unwrap();
    }
}
