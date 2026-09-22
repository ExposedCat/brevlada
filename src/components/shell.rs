use super::{button, column, label, pane, scroll, states};
use crate::theme;
use adw::prelude::*;

pub struct Shell {
    pub window: adw::ApplicationWindow,
    pub sidebar: gtk::Box,
    pub account_sidebar: super::motion::Sidebar,
    pub thread_sidebar: super::motion::Sidebar,
    pub thread_list: gtk::ListBox,
    pub thread_scroll: gtk::ScrolledWindow,
    pub thread_stack: gtk::Stack,
    pub sync_status: super::sync_status::SyncStatus,
    pub list: gtk::ListBox,
    pub list_scroll: gtk::ScrolledWindow,
    pub viewer_scroll: gtk::ScrolledWindow,
    pub list_stack: gtk::Stack,
    pub viewer: gtk::Box,
    pub viewer_reveal: super::reveal::Reveal,
    pub compose_button: gtk::Button,
    pub compose: super::compose::Compose,
    pub refresh: gtk::Button,
    pub sync: gtk::Button,
    pub back: gtk::Button,
    pub search: gtk::SearchEntry,
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
        let sync_status = super::sync_status::SyncStatus::new();
        let sidebar_header = adw::HeaderBar::builder()
            .title_widget(&sync_status.widget)
            .show_end_title_buttons(false)
            .width_request(theme::SIDEBAR_HEADER_WIDTH)
            .css_classes(["sidebar-header"])
            .build();
        let sync = button("view-refresh-symbolic", "Sync accounts now");
        sync.set_sensitive(false);
        sidebar_header.pack_start(&sync);
        let collapse_sidebar = button("sidebar-show-symbolic", "Collapse sidebar");
        sidebar_header.pack_end(&collapse_sidebar);
        sidebar_column.append(&sidebar_header);
        let sidebar = column("navigation-list");
        let sidebar_wrapper = column("sidebar");
        sidebar_wrapper.append(&scroll(&sidebar));
        sidebar_column.append(&sidebar_wrapper);
        let middle = column("message-list-wrapper");
        let list_header = adw::HeaderBar::builder()
            .title_widget(&gtk::Box::new(gtk::Orientation::Horizontal, 0))
            .show_end_title_buttons(false)
            .width_request(theme::LIST_WIDTH)
            .css_classes(["message-list-header"])
            .build();
        let compose_button = button("mail-message-new-symbolic", "Compose message");
        let back = button("go-previous-symbolic", "Back to senders");
        back.set_visible(false);

        let expand_sidebar = button("sidebar-show-symbolic", "Expand sidebar");
        list_header.pack_start(&expand_sidebar);
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
            .activate_on_single_click(true)
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
        list_stack.add_named(&super::sender_menu::container(&list_scroll), Some("list"));
        states::list_state(&list_stack, "No messages in this folder", false, false);
        middle.append(&list_stack);
        let thread_sidebar = column("message-list-wrapper");
        let thread_header = adw::HeaderBar::builder()
            .title_widget(&gtk::Label::new(Some("Messages")))
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .width_request(theme::LIST_WIDTH)
            .css_classes(["message-list-header"])
            .build();
        thread_header.pack_start(&back);
        thread_sidebar.append(&thread_header);
        let thread_list = gtk::ListBox::builder()
            .activate_on_single_click(true)
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["boxed-list"])
            .build();
        let thread_root = adw::PreferencesGroup::builder()
            .hexpand(true)
            .vexpand(true)
            .css_classes(["message-list-root"])
            .build();
        thread_root.add(&thread_list);
        let thread_scroll = scroll(&thread_root);
        let thread_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
        thread_stack.add_named(&super::sender_menu::container(&thread_scroll), Some("list"));
        thread_sidebar.append(&thread_stack);
        let right = column("content-wrapper");
        let header = adw::HeaderBar::builder()
            .title_widget(&gtk::Box::new(gtk::Orientation::Horizontal, 0))
            .centering_policy(adw::CenteringPolicy::Strict)
            .hexpand(true)
            .css_classes(["content-header"])
            .build();
        header.pack_start(&compose_button);
        let headers = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);
        headers.add_widget(&sidebar_header);
        headers.add_widget(&list_header);
        headers.add_widget(&header);
        headers.add_widget(&thread_header);
        right.append(&header);
        let viewer = column("message-container");
        states::select_message(&viewer);
        let viewer_root = column("message-viewer-root");
        viewer_root.set_hexpand(true);
        viewer_root.set_vexpand(true);
        let compose = super::compose::Compose::new(&compose_button);
        let viewer_content = column("message-viewer-content");
        viewer_content.append(&compose.widget);
        viewer_content.append(&viewer);
        let viewer_viewport = gtk::Viewport::builder()
            .child(&viewer_content)
            .scroll_to_focus(false)
            .build();
        let viewer_scroll = scroll(&viewer_viewport);
        let viewer_reveal = super::reveal::Reveal::new(&viewer_scroll);
        viewer_root.append(&viewer_reveal);
        right.append(&viewer_root);
        let thread_sidebar =
            super::motion::Sidebar::new(&thread_sidebar, &right, theme::LIST_WIDTH, false);
        let content = pane(&middle, thread_sidebar.pane(), theme::LIST_WIDTH);
        let account_sidebar =
            super::motion::Sidebar::new(&sidebar_column, &content, theme::SIDEBAR_WIDTH, true);
        let main = account_sidebar.pane();
        super::pane_state::remember(&window, &account_sidebar, &content, &thread_sidebar);
        expand_sidebar.set_visible(!account_sidebar.get_visible());
        let expand = expand_sidebar.downgrade();
        account_sidebar.connect_visible_notify(move |sidebar| {
            if let Some(expand) = expand.upgrade() {
                expand.set_visible(!sidebar.get_visible());
            }
        });
        let target = account_sidebar.downgrade();
        let expand = expand_sidebar.downgrade();
        collapse_sidebar.connect_clicked(move |_| {
            if let Some(target) = target.upgrade() {
                target.set_visible(false);
            }
            if let Some(expand) = expand.upgrade() {
                expand.grab_focus();
            }
        });
        let target = account_sidebar.downgrade();
        let collapse = collapse_sidebar.downgrade();
        expand_sidebar.connect_clicked(move |_| {
            if let Some(target) = target.upgrade() {
                target.set_visible(true);
            }
            if let Some(collapse) = collapse.upgrade() {
                collapse.grab_focus();
            }
        });
        let toolbar = adw::ToolbarView::builder()
            .content(main)
            .top_bar_style(adw::ToolbarStyle::Flat)
            .build();
        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&toolbar));
        window.set_content(Some(&toast));
        Self {
            window,
            sidebar,
            account_sidebar,
            thread_sidebar,
            thread_list,
            thread_scroll,
            thread_stack,
            sync_status,
            list,
            list_scroll,
            viewer_scroll,
            list_stack,
            viewer,
            viewer_reveal,
            compose_button,
            compose,
            refresh,
            sync,
            back,
            search,
            toast,
        }
    }
}

pub fn no_accounts(sidebar: &gtk::Box) {
    sidebar.append(&label("No accounts found", "dim-label"));
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use std::time::{Duration, Instant};

    fn settle(window: &adw::ApplicationWindow, fullscreen: bool, maximized: bool) {
        let context = gtk::glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        eprintln!(
            "requested fullscreen={fullscreen} maximized={maximized}; actual fullscreen={} maximized={} size={}x{}",
            window.is_fullscreen(),
            window.is_maximized(),
            window.width(),
            window.height()
        );
        assert_eq!(window.is_fullscreen(), fullscreen);
        assert_eq!(window.is_maximized(), maximized);
    }

    fn check_shell(shell: &Shell) {
        let window = &shell.window;
        {
            let widget = shell.refresh.upcast_ref::<gtk::Widget>();
            let bounds = widget.compute_bounds(window).unwrap();
            assert!(bounds.x() >= 0.0 && bounds.y() >= 0.0, "{bounds:?}");
            assert!(
                bounds.x() + bounds.width() <= window.width() as f32,
                "{bounds:?}"
            );
            assert!(
                bounds.y() + bounds.height() <= window.height() as f32,
                "{bounds:?}"
            );
        }
        let bounds = shell.refresh.compute_bounds(window).unwrap();
        let picked = window
            .pick(
                f64::from(bounds.x() + bounds.width() / 2.0),
                f64::from(bounds.y() + bounds.height() / 2.0),
                gtk::PickFlags::DEFAULT,
            )
            .unwrap();
        assert!(
            picked == shell.refresh || picked.is_ancestor(&shell.refresh),
            "Picked {} instead of refresh",
            picked.type_().name()
        );
        assert_eq!(gtk::Window::list_toplevels().len(), 1);
    }

    #[test]
    #[ignore = "Requires a graphical session; presents an isolated shell without mail workers"]
    fn empty_state_labels_align() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaEmptyStateDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = Shell::new(&app);
        shell.window.present();
        let context = gtk::glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        let list_state = shell.list_stack.child_by_name("state").unwrap();
        let viewer_state = shell.viewer.first_child().unwrap();
        let list_label = list_state.last_child().unwrap();
        let viewer_label = viewer_state.last_child().unwrap();
        let list_bounds = list_label.compute_bounds(&shell.window).unwrap();
        let viewer_bounds = viewer_label.compute_bounds(&shell.window).unwrap();
        assert!(
            (list_bounds.y() - viewer_bounds.y()).abs() <= 1.0,
            "Empty labels must align: list={list_bounds:?}, viewer={viewer_bounds:?}"
        );
        shell.window.destroy();
    }

    #[test]
    #[ignore = "Requires a graphical session; presents an isolated shell without mail workers"]
    fn window_transitions_keep_headers_and_input_in_sync() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaWindowDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = Shell::new(&app);
        let sidebar_pane = shell
            .sidebar
            .ancestor(gtk::Paned::static_type())
            .and_downcast::<gtk::Paned>()
            .unwrap();
        sidebar_pane.set_position(1106);
        shell.refresh.set_sensitive(true);
        shell.window.present();
        settle(&shell.window, false, false);
        check_shell(&shell);
        for _ in 0..4 {
            shell.window.fullscreen();
            settle(&shell.window, true, false);
            check_shell(&shell);
            shell.window.unfullscreen();
            settle(&shell.window, false, false);
            check_shell(&shell);
            shell.window.maximize();
            settle(&shell.window, false, true);
            check_shell(&shell);
            shell.window.unmaximize();
            settle(&shell.window, false, false);
            check_shell(&shell);
        }
        shell.window.destroy();
    }
}
