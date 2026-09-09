use crate::theme;
use adw::prelude::*;
use webkit6::prelude::*;

const WORLD: &str = "brevlada-layout";
const SCRIPT: &str = concat!(
    include_str!("html_viewer/text_quotes.js"),
    include_str!("html_viewer/quotes.js"),
    include_str!("html_viewer/layout.js"),
);

pub fn connect(view: &webkit6::WebView, manager: &webkit6::UserContentManager) {
    manager.register_script_message_handler("bodySize", Some(WORLD));
    let weak = view.downgrade();
    manager.connect_script_message_received(Some("bodySize"), move |_, value| {
        if let Some(view) = weak.upgrade() {
            let height = value.to_int32().max(theme::BODY_HEIGHT);
            if height == view.height_request() {
                return;
            }
            if let Some(scroll) = view
                .ancestor(gtk::ScrolledWindow::static_type())
                .and_downcast::<gtk::ScrolledWindow>()
                && let Some(container) = scroll.child()
            {
                super::scroll_position::preserve(&scroll, &container, || {
                    view.set_height_request(height)
                });
            } else {
                view.set_height_request(height);
            }
        }
    });
}

pub fn observe(view: &webkit6::WebView) {
    view.evaluate_javascript(
        SCRIPT,
        Some(WORLD),
        None,
        gtk::gio::Cancellable::NONE,
        |result| {
            if let Err(error) = result {
                eprintln!("Could not measure message content: {error}");
            }
        },
    );
}
