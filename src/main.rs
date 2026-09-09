mod backend;
mod components;
mod models;
mod theme;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    gio::resources_register_include!("brevlada.gresource").expect("Bundled resources are invalid");
    let app = adw::Application::builder()
        .application_id("org.example.OnlineAccounts")
        .build();
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
        } else {
            window::create(app);
        }
    });
    app.run()
}
