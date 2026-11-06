use crate::theme;
use adw::prelude::*;
use gtk::gio;

pub fn menu() -> (gtk::MenuButton, gio::SimpleAction) {
    let settings = gio::SimpleAction::new("settings", None);
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&settings);
    let menu = gio::Menu::new();
    menu.append(Some("Settings"), Some("window.settings"));
    menu.append(Some("Quit"), Some("app.quit"));
    let button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main menu")
        .menu_model(&menu)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    button.insert_action_group("window", Some(&actions));
    (button, settings)
}

pub struct SettingsWindow {
    pub window: adw::Window,
    pub toasts: adw::ToastOverlay,
    pub trust_all_senders: adw::SwitchRow,
    pub accounts: std::rc::Rc<super::account_settings::Accounts>,
    pub accounts_section: adw::ActionRow,
    pub sync: std::rc::Rc<super::sync_settings::SyncSettings>,
    pub sync_section: adw::ActionRow,
}

impl SettingsWindow {
    pub fn new(parent: &impl IsA<gtk::Window>, trust_all_senders: bool) -> Self {
        let window = adw::Window::builder()
            .title("Settings")
            .transient_for(parent)
            .modal(true)
            .destroy_with_parent(true)
            .default_width(theme::SETTINGS_WIDTH)
            .default_height(theme::SETTINGS_HEIGHT)
            .width_request(theme::SETTINGS_MIN_WIDTH)
            .height_request(theme::SETTINGS_MIN_HEIGHT)
            .build();
        window.set_application(parent.as_ref().application().as_ref());
        let navigation = super::settings_navigation::SettingsNavigation::new(&window);
        let privacy = adw::PreferencesPage::builder()
            .title("Privacy")
            .icon_name("changes-prevent-symbolic")
            .build();
        let group = adw::PreferencesGroup::new();
        let trust_all_senders = adw::SwitchRow::builder()
            .title("Trust all senders")
            .subtitle("Automatically load message media from all senders")
            .active(trust_all_senders)
            .build();
        group.add(&trust_all_senders);
        privacy.add(&group);
        let general = adw::PreferencesPage::builder()
            .title("General")
            .icon_name("preferences-system-symbolic")
            .build();
        let background = adw::SwitchRow::builder()
            .title("Keep checking when the window is closed")
            .subtitle("Continue checking for new mail and sending notifications")
            .action_name("app.background")
            .build();
        let group = adw::PreferencesGroup::new();
        group.add(&background);
        let autostart = adw::SwitchRow::builder()
            .title("Start at login")
            .subtitle("Start checking for mail without opening a window")
            .action_name("app.autostart")
            .build();
        group.add(&autostart);
        general.add(&group);
        navigation.add_section("General", "preferences-system-symbolic", &general);
        let accounts = super::account_settings::Accounts::new();
        let accounts_section =
            navigation.add_section("Accounts", "mail-unread-symbolic", &accounts.page);
        let sync = super::sync_settings::SyncSettings::new();
        let sync_section =
            navigation.add_section("Automatic sync", "view-refresh-symbolic", &sync.page);
        navigation.add_section("Privacy", "changes-prevent-symbolic", &privacy);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&navigation.split));
        window.set_content(Some(&toasts));
        Self {
            window,
            toasts,
            trust_all_senders,
            accounts,
            accounts_section,
            sync,
            sync_section,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a graphical session"]
    fn background_switch_uses_application_actions() {
        gtk::init().unwrap();
        let app = adw::Application::builder()
            .application_id("com.chipichapa.Brevlada.BackgroundDiagnostics")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let action = gio::SimpleAction::new_stateful("background", None, &false.to_variant());
        let autostart = gio::SimpleAction::new_stateful("autostart", None, &true.to_variant());
        app.add_action(&action);
        app.add_action(&autostart);
        let parent = adw::ApplicationWindow::new(&app);
        let settings = SettingsWindow::new(&parent, false);
        let section = find_row(settings.window.clone().upcast(), "General").unwrap();
        section.emit_by_name::<()>("activated", &[]);
        let switch = find_row(
            settings.window.clone().upcast(),
            "Keep checking when the window is closed",
        )
        .unwrap()
        .downcast::<adw::SwitchRow>()
        .unwrap();
        assert!(!switch.is_active());
        let startup_switch = find_row(settings.window.clone().upcast(), "Start at login")
            .unwrap()
            .downcast::<adw::SwitchRow>()
            .unwrap();
        assert!(startup_switch.is_active());
        startup_switch
            .activate_action("app.autostart", None)
            .unwrap();
        assert!(!startup_switch.is_active());
        assert_eq!(autostart.state().unwrap().get::<bool>(), Some(false));
        assert!(!switch.is_active());
        switch.activate_action("app.background", None).unwrap();
        assert_eq!(action.state().unwrap().get::<bool>(), Some(true));
        assert!(switch.is_active());
        assert!(!startup_switch.is_active());
        action.set_state(&false.to_variant());
        assert!(!switch.is_active());
        action.set_enabled(false);
        assert!(!switch.is_sensitive());
        settings.window.destroy();
        parent.destroy();
    }

    fn find_row(widget: gtk::Widget, title: &str) -> Option<adw::ActionRow> {
        if let Some(row) = widget.downcast_ref::<adw::ActionRow>()
            && row.title() == title
        {
            return Some(row.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            if let Some(row) = find_row(widget, title) {
                return Some(row);
            }
        }
        None
    }
}
