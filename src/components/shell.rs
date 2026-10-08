use super::{button, column, label, pane, scroll, states};
use crate::theme;
use adw::prelude::*;

pub struct Shell {
    pub window: adw::ApplicationWindow,
    pub sidebar: gtk::Box,
    pub account_sidebar: super::motion::Sidebar,
    pub account_title: gtk::Label,
    pub thread_sidebar: super::motion::Sidebar,
    pub thread_list: gtk::ListView,
    pub thread_scroll: gtk::ScrolledWindow,
    pub thread_stack: gtk::Stack,
    pub sync_status: super::sync_status::SyncStatus,
    pub list: gtk::ListView,
    pub list_scroll: gtk::ScrolledWindow,
    pub thread_load_more: gtk::Button,
    pub viewer_scroll: gtk::ScrolledWindow,
    pub list_stack: gtk::Stack,
    pub viewer: gtk::Box,
    pub viewer_reveal: super::reveal::Reveal,
    pub compose_button: gtk::Button,
    pub composers: gtk::Box,
    pub refresh: gtk::Button,
    pub sync: gtk::Button,
    pub back: gtk::Button,
    pub sender_unread_first: super::sort_menu::SortMenu,
    pub thread_unread_first: super::sort_menu::SortMenu,
    pub search: gtk::SearchEntry,
    pub search_progress: [gtk::Spinner; 2],
    pub search_filters: std::rc::Rc<super::search_filters::SearchFilters>,
    pub thread_search: gtk::SearchEntry,
    pub toast: adw::ToastOverlay,
    pub settings: gtk::gio::SimpleAction,
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
        let account_title = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(40)
            .css_classes(["title"])
            .visible(false)
            .build();
        let list_header = adw::HeaderBar::builder()
            .title_widget(&account_title)
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
        middle.append(&list_header);
        let search_filters = super::search_filters::SearchFilters::new();
        let search = search_controls(
            &list_header,
            &middle,
            "Search senders and messages",
            Some(&search_filters.widget),
        );
        let sender_unread_first = super::sort_menu::SortMenu::new();
        list_header.pack_end(&sender_unread_first.widget);
        let list = super::virtual_list::new();
        let list_scroll = super::virtual_list::scroll(&list);
        let search_progress = std::array::from_fn(|_| {
            let spinner = gtk::Spinner::builder()
                .spinning(true)
                .visible(false)
                .halign(gtk::Align::Center)
                .height_request(20)
                .width_request(20)
                .margin_top(6)
                .margin_bottom(6)
                .build();
            spinner.set_tooltip_text(Some("Searching…"));
            spinner
        });
        middle.append(&search_progress[0]);
        let list_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
        let list_root = super::sender_menu::container(&list_scroll);
        list_root.add_css_class("message-list-root");
        list_stack.add_named(&list_root, Some("list"));
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
        let thread_unread_first = super::sort_menu::SortMenu::new();
        thread_header.pack_end(&thread_unread_first.widget);
        thread_sidebar.append(&thread_header);
        let thread_search = search_controls(
            &thread_header,
            &thread_sidebar,
            "Search this sender’s messages",
            None,
        );
        thread_sidebar.append(&search_progress[1]);
        let thread_list = super::virtual_list::new();
        let thread_load_more = gtk::Button::with_label("Retry loading");
        thread_load_more.add_css_class("message-list-more");
        thread_load_more.set_halign(gtk::Align::Center);
        thread_load_more.set_valign(gtk::Align::End);
        thread_load_more.set_margin_bottom(theme::SMALL_SPACING);
        thread_load_more.set_visible(false);
        let thread_scroll = super::virtual_list::scroll(&thread_list);
        let thread_overlay = gtk::Overlay::new();
        thread_overlay.set_hexpand(true);
        thread_overlay.set_vexpand(true);
        thread_overlay.set_child(Some(&thread_scroll));
        thread_overlay.add_overlay(&thread_load_more);
        let thread_root = super::sender_menu::container(&thread_overlay);
        thread_root.add_css_class("message-list-root");
        let thread_stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
        thread_stack.add_named(&thread_root, Some("list"));
        thread_sidebar.append(&thread_stack);
        let right = column("content-wrapper");
        let header = adw::HeaderBar::builder()
            .title_widget(&gtk::Box::new(gtk::Orientation::Horizontal, 0))
            .centering_policy(adw::CenteringPolicy::Strict)
            .hexpand(true)
            .css_classes(["content-header"])
            .build();
        header.pack_start(&compose_button);
        let (menu, settings) = super::settings::menu();
        header.pack_end(&menu);
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
        let composers = column("compose-container");
        let viewer_content = column("message-viewer-content");
        viewer_content.append(&composers);
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
        let title = account_title.downgrade();
        account_sidebar.connect_visible_notify(move |sidebar| {
            if let Some(expand) = expand.upgrade() {
                expand.set_visible(!sidebar.get_visible());
            }
            if let Some(title) = title.upgrade() {
                title.set_visible(!sidebar.get_visible() && !title.text().is_empty());
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
            account_title,
            thread_sidebar,
            thread_list,
            thread_scroll,
            thread_stack,
            sync_status,
            list,
            list_scroll,
            thread_load_more,
            viewer_scroll,
            list_stack,
            viewer,
            viewer_reveal,
            compose_button,
            composers,
            refresh,
            sync,
            back,
            sender_unread_first,
            thread_unread_first,
            search,
            search_filters,
            search_progress,
            thread_search,
            toast,
            settings,
        }
    }
}

fn search_controls(
    header: &adw::HeaderBar,
    parent: &gtk::Box,
    prompt: &str,
    filters: Option<&gtk::Box>,
) -> gtk::SearchEntry {
    let toggle = gtk::ToggleButton::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text(prompt)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    header.pack_start(&toggle);
    let entry = gtk::SearchEntry::builder()
        .placeholder_text(prompt)
        .width_chars(1)
        .hexpand(true)
        .halign(gtk::Align::Fill)
        .search_delay(150)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.set_hexpand(true);
    if let Some(filters) = filters {
        content.append(filters);
    }
    content.append(&entry);
    let bar = gtk::SearchBar::builder()
        .child(&content)
        .search_mode_enabled(false)
        .css_classes(["message-list-search-box"])
        .build();
    bar.connect_entry(&entry);
    toggle
        .bind_property("active", &bar, "search-mode-enabled")
        .bidirectional()
        .sync_create()
        .build();
    let focus = entry.clone();
    toggle.connect_toggled(move |toggle| {
        if toggle.is_active() {
            focus.grab_focus();
        }
    });
    parent.append(&bar);
    entry
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
    #[ignore = "Requires a graphical session to inspect search and filter allocation"]
    fn search_entries_fill_their_panes_and_filters_stay_above_sender_search() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSearchDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let shell = Shell::new(&app);
        shell.account_sidebar.set_visible(false);
        shell.thread_sidebar.set_visible(true);
        for entry in [&shell.search, &shell.thread_search] {
            entry
                .ancestor(gtk::SearchBar::static_type())
                .and_downcast::<gtk::SearchBar>()
                .unwrap()
                .set_search_mode(true);
        }
        shell.window.present();
        let context = gtk::glib::MainContext::default();
        let settle = || {
            let deadline = Instant::now() + Duration::from_millis(400);
            while Instant::now() < deadline {
                context.iteration(false);
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        settle();
        for entry in [&shell.search, &shell.thread_search] {
            let bar = entry.ancestor(gtk::SearchBar::static_type()).unwrap();
            let bounds = entry.compute_bounds(&shell.window).unwrap();
            let bar_bounds = bar.compute_bounds(&shell.window).unwrap();
            eprintln!("search={bounds:?}, bar={bar_bounds:?}");
            assert!(
                bar_bounds.width() - bounds.width() <= 16.0,
                "Search must fill its pane: {} vs {}",
                entry.width(),
                bar.width()
            );
        }
        let filters = shell
            .search_filters
            .widget
            .compute_bounds(&shell.window)
            .unwrap();
        let entry = shell.search.compute_bounds(&shell.window).unwrap();
        assert!(filters.y() + filters.height() <= entry.y());
        if let Ok(path) = std::env::var("BREVLADA_SEARCH_SCREENSHOT") {
            let paintable = gtk::WidgetPaintable::new(Some(&shell.window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(
                &snapshot,
                shell.window.width() as f64,
                shell.window.height() as f64,
            );
            shell
                .window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(path)
                .unwrap();
        }
        // Resizing the list should resize both the entry and filter controls.
        let pane = shell
            .search
            .ancestor(gtk::Paned::static_type())
            .and_downcast::<gtk::Paned>()
            .unwrap();
        pane.set_position(550);
        settle();
        assert!(pane.position() >= 550);
        let bar = shell
            .search
            .ancestor(gtk::SearchBar::static_type())
            .unwrap();
        assert!(
            bar.compute_bounds(&shell.window).unwrap().width()
                - shell.search.compute_bounds(&shell.window).unwrap().width()
                <= 16.0
        );
        shell.window.destroy();
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
