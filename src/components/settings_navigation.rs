use crate::theme;
use adw::prelude::*;

pub struct SettingsNavigation {
    pub split: adw::NavigationSplitView,
    sections: gtk::ListBox,
    stack: gtk::Stack,
    content: adw::NavigationPage,
}

impl SettingsNavigation {
    pub fn new(window: &adw::Window) -> Self {
        let sections = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&sections)
            .build();
        let sidebar = adw::ToolbarView::builder().content(&scroll).build();
        sidebar.add_css_class("sidebar");
        sidebar.add_top_bar(&adw::HeaderBar::new());
        let stack = gtk::Stack::builder().hexpand(true).vexpand(true).build();
        let toolbar = adw::ToolbarView::builder().content(&stack).build();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        let content = adw::NavigationPage::new(&toolbar, "General");
        let split = adw::NavigationSplitView::builder()
            .sidebar(&adw::NavigationPage::new(&sidebar, "Settings"))
            .content(&content)
            .min_sidebar_width(f64::from(theme::SETTINGS_SIDEBAR_WIDTH))
            .max_sidebar_width(f64::from(theme::SETTINGS_SIDEBAR_WIDTH))
            .show_content(true)
            .build();
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            f64::from(theme::SETTINGS_NARROW_WIDTH),
            adw::LengthUnit::Px,
        ));
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(breakpoint);
        Self {
            split,
            sections,
            stack,
            content,
        }
    }

    pub fn add_section(
        &self,
        title: &str,
        icon: &str,
        content: &impl IsA<gtk::Widget>,
    ) -> adw::ActionRow {
        self.stack.add_named(content, Some(title));
        let row = adw::ActionRow::builder()
            .title(title)
            .activatable(true)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(icon));
        let next = gtk::Image::from_icon_name("go-next-symbolic");
        row.add_suffix(&next);
        self.split
            .bind_property("collapsed", &next, "visible")
            .sync_create()
            .build();
        let stack = self.stack.downgrade();
        let split = self.split.downgrade();
        let page = self.content.downgrade();
        let sections = self.sections.downgrade();
        row.connect_activated(move |row| {
            if let (Some(stack), Some(split), Some(page), Some(sections)) = (
                stack.upgrade(),
                split.upgrade(),
                page.upgrade(),
                sections.upgrade(),
            ) {
                sections.select_row(Some(row));
                stack.set_visible_child_name(&row.title());
                page.set_title(&row.title());
                split.set_show_content(true);
            }
        });
        self.sections.append(&row);
        if self.sections.selected_row().is_none() {
            self.sections.select_row(Some(&row));
            self.stack.set_visible_child_name(title);
            self.content.set_title(title);
        }
        row
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn layout() {
        let context = gtk::glib::MainContext::default();
        let until = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while std::time::Instant::now() < until {
            context.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn wait(predicate: impl Fn() -> bool) {
        let context = gtk::glib::MainContext::default();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !predicate() && std::time::Instant::now() < until {
            context.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(predicate());
    }

    fn row(widget: &gtk::Widget, title: &str) -> Option<adw::ActionRow> {
        if let Some(row) = widget.downcast_ref::<adw::ActionRow>()
            && row.title() == title
        {
            return Some(row.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(row) = row(&widget, title) {
                return Some(row);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn preview(window: &adw::Window, name: &str) {
        if let Some(path) = std::env::var_os("BREVLADA_SETTINGS_NAVIGATION_PREVIEW") {
            let paintable = gtk::WidgetPaintable::new(Some(window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(format!("{}-{name}.png", path.to_string_lossy()))
                .unwrap();
        }
    }

    #[test]
    #[ignore = "Requires a private graphical session; settings fixtures without mail workers"]
    fn sections_are_a_sidebar_when_wide_and_back_navigation_when_narrow() {
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.example.BrevladaSettingsNavigationDiagnostic")
            .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gtk::gio::Cancellable::NONE).unwrap();
        let parent = adw::ApplicationWindow::new(&app);
        let settings = super::super::settings::SettingsWindow::new(&parent, false);
        let split = settings
            .toasts
            .child()
            .unwrap()
            .downcast::<adw::NavigationSplitView>()
            .unwrap();
        settings.window.set_default_size(1200, 800);
        settings.window.present();
        layout();
        assert!(!split.is_collapsed());
        assert!(split.sidebar().unwrap().is_mapped());
        assert!(split.content().unwrap().is_mapped());
        row(settings.window.upcast_ref(), "Privacy")
            .unwrap()
            .emit_by_name::<()>("activated", &[]);
        row(settings.window.upcast_ref(), "Privacy")
            .unwrap()
            .grab_focus();
        settings.trust_all_senders.set_active(true);
        layout();
        assert_eq!(split.content().unwrap().title(), "Privacy");
        preview(&settings.window, "wide");
        settings.window.set_default_size(560, 600);
        layout();
        assert!(split.is_collapsed());
        assert!(split.shows_content());
        assert!(!split.sidebar().unwrap().is_mapped());
        assert!(settings.trust_all_senders.is_mapped());
        assert!(settings.trust_all_senders.is_active());
        preview(&settings.window, "narrow-content");
        split.set_show_content(false);
        wait(|| !split.content().unwrap().is_mapped());
        layout();
        assert!(split.sidebar().unwrap().is_mapped());
        assert!(!split.content().unwrap().is_mapped());
        preview(&settings.window, "narrow-sections");
        settings
            .accounts_section
            .emit_by_name::<()>("activated", &[]);
        layout();
        assert!(split.shows_content());
        assert_eq!(split.content().unwrap().title(), "Accounts");
        settings.window.set_default_size(1200, 800);
        layout();
        assert!(!split.is_collapsed());
        assert!(split.sidebar().unwrap().is_mapped());
        assert!(settings.trust_all_senders.is_active());
        assert_eq!(split.content().unwrap().title(), "Accounts");
        settings.window.destroy();
        parent.destroy();
    }
}
