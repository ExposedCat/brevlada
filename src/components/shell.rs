use super::{button, column, label, pane, scroll, states};
use crate::theme;
use adw::prelude::*;

pub struct Shell {
    pub window: adw::ApplicationWindow,
    pub sidebar: gtk::Box,
    pub list: gtk::ListBox,
    pub list_scroll: gtk::ScrolledWindow,
    pub viewer_scroll: gtk::ScrolledWindow,
    pub list_stack: gtk::Stack,
    pub viewer: gtk::Box,
    pub refresh: gtk::Button,
    pub search: gtk::SearchEntry,
    pub list_title: gtk::Label,
    pub content_title: adw::WindowTitle,
    pub toast: adw::ToastOverlay,
}

impl Shell {
    pub fn new(app: &adw::Application) -> Self {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Brevlada Email Client")
            .default_width(theme::WINDOW_WIDTH)
            .default_height(theme::WINDOW_HEIGHT)
            .build();
        let display = gtk::prelude::WidgetExt::display(&window);
        let css = gtk::CssProvider::new();
        css.load_from_string(theme::CSS);
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        gtk::IconTheme::for_display(&display).add_resource_path("/org/gtk/example/icons");
        let sidebar_column = column("sidebar-wrapper");
        let sidebar_header = adw::HeaderBar::builder()
            .title_widget(&gtk::Label::new(Some("Accounts")))
            .show_end_title_buttons(false)
            .width_request(theme::SIDEBAR_HEADER_WIDTH)
            .css_classes(["sidebar-header"])
            .build();
        sidebar_column.append(&sidebar_header);
        let sidebar = column("navigation-list");
        let sidebar_wrapper = column("sidebar");
        sidebar_wrapper.append(&scroll(&sidebar));
        sidebar_column.append(&sidebar_wrapper);
        let middle = column("message-list-wrapper");
        let list_title = gtk::Label::new(Some("Messages"));
        let list_header = adw::HeaderBar::builder()
            .title_widget(&list_title)
            .show_end_title_buttons(false)
            .width_request(theme::LIST_WIDTH)
            .css_classes(["message-list-header"])
            .build();
        let refresh = button("view-refresh-symbolic", "Refresh messages");
        refresh.set_sensitive(false);
        list_header.pack_start(&refresh);
        let search_toggle = gtk::ToggleButton::builder()
            .icon_name("system-search-symbolic")
            .tooltip_text("Search messages")
            .hexpand(false)
            .vexpand(false)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        search_toggle.add_css_class("flat");
        list_header.pack_end(&search_toggle);
        middle.append(&list_header);
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search messages")
            .width_chars(25)
            .max_width_chars(40)
            .build();
        let search_bar = gtk::SearchBar::builder()
            .child(&search)
            .search_mode_enabled(false)
            .css_classes(["message-list-search-box"])
            .build();
        search_bar.connect_entry(&search);
        search_toggle
            .bind_property("active", &search_bar, "search-mode-enabled")
            .bidirectional()
            .sync_create()
            .build();
        let entry = search.clone();
        search_toggle.connect_toggled(move |toggle| {
            if toggle.is_active() {
                entry.grab_focus();
            }
        });
        middle.append(&search_bar);
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["boxed-list"])
            .build();
        let list_root = adw::PreferencesGroup::builder()
            .hexpand(true)
            .vexpand(true)
            .css_classes(["message-list-root"])
            .build();
        list_root.add(&list);
        let list_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
        let list_scroll = scroll(&list_root);
        list_stack.add_named(&list_scroll, Some("list"));
        states::list_state(&list_stack, "No messages in this folder", false, false);
        middle.append(&list_stack);
        let right = column("content-wrapper");
        let content_title = adw::WindowTitle::new("Online Accounts", "");
        let header = adw::HeaderBar::builder()
            .title_widget(&content_title)
            .centering_policy(adw::CenteringPolicy::Strict)
            .hexpand(true)
            .css_classes(["content-header"])
            .build();
        right.append(&header);
        let viewer = column("message-container");
        states::select_message(&viewer);
        let viewer_root = column("message-viewer-root");
        viewer_root.set_hexpand(true);
        viewer_root.set_vexpand(true);
        let viewer_viewport = gtk::Viewport::builder()
            .child(&viewer)
            .scroll_to_focus(false)
            .build();
        let viewer_scroll = scroll(&viewer_viewport);
        viewer_root.append(&viewer_scroll);
        right.append(&viewer_root);
        let content = pane(&middle, &right, theme::LIST_WIDTH);
        let main = pane(&sidebar_column, &content, theme::SIDEBAR_WIDTH);
        let toolbar = adw::ToolbarView::builder()
            .content(&main)
            .top_bar_style(adw::ToolbarStyle::Flat)
            .build();
        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&toolbar));
        window.set_content(Some(&toast));
        Self {
            window,
            sidebar,
            list,
            list_scroll,
            viewer_scroll,
            list_stack,
            viewer,
            refresh,
            search,
            list_title,
            content_title,
            toast,
        }
    }
}

pub fn no_accounts(sidebar: &gtk::Box) {
    sidebar.append(&label("No accounts found", "dim-label"));
}
