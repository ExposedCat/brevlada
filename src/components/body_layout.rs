use crate::theme;
use adw::prelude::*;
use webkit6::prelude::*;

const WORLD: &str = "brevlada-layout";
const SCRIPT: &str = concat!(
    include_str!("html_viewer/text_quotes.js"),
    include_str!("html_viewer/quotes.js"),
    include_str!("html_viewer/layout.js"),
);

pub fn connect(
    view: &webkit6::WebView,
    content: &gtk::ScrolledWindow,
    manager: &webkit6::UserContentManager,
    ready: impl Fn() + 'static,
) {
    manager.register_script_message_handler("bodySize", Some(WORLD));
    manager.add_script(&webkit6::UserScript::for_world(
        SCRIPT,
        webkit6::UserContentInjectedFrames::TopFrame,
        webkit6::UserScriptInjectionTime::End,
        WORLD,
        &[],
        &[],
    ));
    let weak = view.downgrade();
    let target = content.downgrade();
    manager.connect_script_message_received(Some("bodySize"), move |_, value| {
        if let (Some(view), Some(content)) = (weak.upgrade(), target.upgrade()) {
            let height = value.to_int32().max(theme::BODY_HEIGHT);
            if height == view.height_request() {
                ready();
                return;
            }
            if let Some(scroll) = content
                .ancestor(gtk::ScrolledWindow::static_type())
                .and_downcast::<gtk::ScrolledWindow>()
                && let Some(container) = scroll.child()
            {
                super::scroll_position::preserve(&scroll, &container, || {
                    view.set_height_request(height);
                    content.set_height_request(height);
                });
            } else {
                view.set_height_request(height);
                content.set_height_request(height);
            }
            ready();
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
