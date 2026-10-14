use adw::prelude::*;
use gtk::{gio, glib};

pub fn install(app: &adw::Application, headless: impl Fn() + 'static) {
    app.set_flags(app.flags() | gio::ApplicationFlags::HANDLES_COMMAND_LINE);
    app.add_main_option(
        "background",
        glib::Char::from(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        "Check mail without opening a window",
        None,
    );
    app.connect_command_line(move |app, command| {
        if command
            .options_dict()
            .lookup::<bool>("background")
            .ok()
            .flatten()
            .unwrap_or(false)
        {
            headless();
        } else {
            app.activate();
        }
        glib::ExitCode::SUCCESS
    });
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    #[ignore = "requires a graphical session; no mail workers or autostart changes"]
    fn login_command_does_not_activate_or_create_a_window() {
        gtk::init().unwrap();
        let app = adw::Application::builder()
            .application_id("com.chipichapa.Brevlada.StartupDiagnostics")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        let activated = Rc::new(Cell::new(false));
        let observed = activated.clone();
        app.connect_activate(move |_| observed.set(true));
        let started = Rc::new(Cell::new(false));
        let observed = started.clone();
        install(&app, move || observed.set(true));
        assert_eq!(
            app.run_with_args(&["brevlada", "--background"]),
            glib::ExitCode::SUCCESS
        );
        assert!(started.get());
        assert!(!activated.get());
        assert!(app.windows().is_empty());
    }
}
