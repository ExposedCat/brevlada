use crate::theme;
use adw::prelude::*;
use gtk::gio;

pub fn menu() -> (gtk::MenuButton, gio::SimpleAction) {
    let settings = gio::SimpleAction::new("settings", None);
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&settings);
    let menu = gio::Menu::new();
    menu.append(Some("Settings"), Some("window.settings"));
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
