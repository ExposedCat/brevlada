mod application;
mod backend;
mod background;
mod components;
mod models;
mod theme;
mod window;

use adw::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn main() -> glib::ExitCode {
    gio::resources_register_include!("brevlada.gresource").expect("Bundled resources are invalid");
    let app = adw::Application::builder()
        .application_id("io.github.ExposedCat.Brevlada")
        .build();
    let engine = Rc::new(RefCell::new(None::<Rc<application::Engine>>));
    let background = Rc::new(RefCell::new(None::<Rc<background::Background>>));
    let startup_engine = engine.clone();
    let startup_background = background.clone();
    app.connect_startup(move |app| {
        let engine = application::Engine::new(app);
        *startup_background.borrow_mut() = Some(background::Background::new(app, &engine));
        *startup_engine.borrow_mut() = Some(engine);
    });
    let activated_engine = engine.clone();
    app.connect_activate(move |app| {
        if let Some(window) = app
            .windows()
            .into_iter()
            .find(|window| window.is::<adw::ApplicationWindow>())
        {
            window.present();
        } else if let Some(engine) = activated_engine.borrow().as_ref() {
            window::create(app, engine);
        }
    });
    let quit = gio::SimpleAction::new("quit", None);
    let quitting = Rc::new(Cell::new(false));
    let removed_quitting = quitting.clone();
    app.connect_window_removed(move |app, _| {
        if removed_quitting.get() && app.windows().is_empty() {
            app.quit();
        }
    });
    let weak = app.downgrade();
    quit.connect_activate(move |_, _| {
        if let Some(app) = weak.upgrade() {
            quitting.set(true);
            let windows = app.windows();
            if windows.is_empty() {
                app.quit();
            }
            for window in windows {
                window.close();
            }
        }
    });
    app.add_action(&quit);
    app.set_accels_for_action("app.quit", &["<Primary>q"]);
    let open = gio::SimpleAction::new(
        "open-message",
        Some(&<(String, u32, u32)>::static_variant_type()),
    );
    let weak = app.downgrade();
    let opened_engine = engine.clone();
    open.connect_activate(move |_, target| {
        if let Some(app) = weak.upgrade()
            && let Some((account, validity, uid)) =
                target.and_then(|target| target.get::<(String, u32, u32)>())
        {
            app.activate();
            app.withdraw_notification(&format!("new-mail-{account}"));
            if let Some(engine) = opened_engine.borrow().as_ref() {
                let _ = engine
                    .worker
                    .send(backend::worker::Command::OpenNotification {
                        account,
                        validity,
                        uid,
                    });
            }
        }
    });
    app.add_action(&open);
    app.connect_shutdown(move |_| {
        if let Some(background) = background.borrow_mut().take() {
            background.stop();
        }
        if let Some(engine) = engine.borrow_mut().take() {
            engine.stop();
        }
    });
    app.run()
}
