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
    pub window: adw::PreferencesWindow,
    pub trust_all_senders: adw::SwitchRow,
}

impl SettingsWindow {
    pub fn new(parent: &impl IsA<gtk::Window>, trust_all_senders: bool) -> Self {
        let window = adw::PreferencesWindow::builder()
            .title("Settings")
            .transient_for(parent)
            .modal(true)
            .destroy_with_parent(true)
            .search_enabled(false)
            .default_width(theme::SETTINGS_WIDTH)
            .default_height(theme::SETTINGS_HEIGHT)
            .build();
        window.set_application(parent.as_ref().application().as_ref());
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
        let overview = adw::PreferencesPage::builder().title("Settings").build();
        let sections = adw::PreferencesGroup::new();
        sections.add(&section(
            &window,
            "Privacy",
            "changes-prevent-symbolic",
            &privacy,
        ));
        overview.add(&sections);
        let general = adw::PreferencesPage::builder()
            .title("General")
            .icon_name("preferences-system-symbolic")
            .build();
        let background = adw::SwitchRow::builder()
            .title("Background service")
            .subtitle("Keep checking for new mail when the window is closed")
            .action_name("app.background")
            .build();
        let group = adw::PreferencesGroup::new();
        group.add(&background);
        general.add(&group);
        sections.add(&section(
            &window,
            "General",
            "preferences-system-symbolic",
            &general,
        ));
        window.add(&overview);
        Self {
            window,
            trust_all_senders,
        }
    }
}

fn section(
    window: &adw::PreferencesWindow,
    title: &str,
    icon: &str,
    content: &adw::PreferencesPage,
) -> adw::ActionRow {
    let toolbar = adw::ToolbarView::builder().content(content).build();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let page = adw::NavigationPage::new(&toolbar, title);
    let row = adw::ActionRow::builder()
        .title(title)
        .activatable(true)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name(icon));
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    let window = window.downgrade();
    row.connect_activated(move |_| {
        if let Some(window) = window.upgrade() {
            window.push_subpage(&page);
        }
    });
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a graphical session"]
    fn background_switch_uses_application_actions() {
        gtk::init().unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.ExposedCat.Brevlada.BackgroundDiagnostics")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let action = gio::SimpleAction::new_stateful("background", None, &false.to_variant());
        app.add_action(&action);
        let parent = adw::ApplicationWindow::new(&app);
        let settings = SettingsWindow::new(&parent, false);
        let overview = settings.window.visible_page().unwrap();
        let section = find_row(overview.upcast(), "General").unwrap();
        section.emit_by_name::<()>("activated", &[]);
        let switch = find_row(settings.window.clone().upcast(), "Background service")
            .unwrap()
            .downcast::<adw::SwitchRow>()
            .unwrap();
        assert!(!switch.is_active());
        switch.activate_action("app.background", None).unwrap();
        assert_eq!(action.state().unwrap().get::<bool>(), Some(true));
        assert!(switch.is_active());
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
