use crate::{models::Message, theme};
use adw::prelude::*;
use webkit6::prelude::*;

pub fn view(message: &Message, media: bool, hide_quotes: bool) -> (gtk::Widget, webkit6::WebView) {
    let settings = webkit6::Settings::builder()
        .enable_javascript(true)
        .enable_javascript_markup(false)
        .enable_html5_local_storage(false)
        .auto_load_images(media)
        .build();
    let manager = webkit6::UserContentManager::new();
    let view = webkit6::WebView::builder()
        .user_content_manager(&manager)
        .vexpand(false)
        .settings(&settings)
        .height_request(theme::BODY_HEIGHT)
        .hexpand(true)
        .build();
    // WebKit's minimum height can exceed a short message's measured content.
    // Keep that minimum from propagating into the card's layout.
    let content = gtk::ScrolledWindow::builder()
        .child(&view)
        .opacity(0.0)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::External)
        .height_request(theme::BODY_HEIGHT)
        .vexpand(false)
        .build();
    let stack = gtk::Stack::builder()
        .vexpand(false)
        .height_request(theme::BODY_HEIGHT)
        .vhomogeneous(false)
        .build();
    stack.add_named(&loading(), Some("loading"));
    stack.set_visible_child_name("loading");
    let overlay = gtk::Overlay::builder()
        .child(&content)
        .vexpand(false)
        .build();
    overlay.add_overlay(&stack);
    overlay.set_measure_overlay(&stack, true);
    let target = stack.downgrade();
    let loaded = content.downgrade();
    super::body_layout::connect(&view, &content, &manager, move || {
        if let Some(stack) = target.upgrade()
            && stack.visible_child_name().as_deref() != Some("error")
            && let Some(content) = loaded.upgrade()
        {
            content.set_opacity(1.0);
            stack.set_visible(false);
        }
    });
    view.connect_load_changed(move |view, event| {
        if event == webkit6::LoadEvent::Finished {
            super::body_layout::observe(view);
        }
    });
    let document: std::rc::Rc<std::cell::RefCell<Option<String>>> = Default::default();
    let target = stack.downgrade();
    let loaded = content.downgrade();
    let html = document.clone();
    view.connect_load_failed(move |view, _, _, failure| {
        if let Some(stack) = target.upgrade() {
            if let Some(content) = loaded.upgrade() {
                content.set_opacity(0.0);
            }
            let weak = view.downgrade();
            let target = stack.downgrade();
            let html = html.clone();
            let row = error(
                &failure.to_string(),
                std::rc::Rc::new(move || {
                    if let (Some(view), Some(stack)) = (weak.upgrade(), target.upgrade()) {
                        stack.set_visible_child_name("loading");
                        if let Some(html) = html.borrow().as_ref() {
                            view.load_html(html, Some("about:blank"));
                        }
                    }
                }),
            );
            if let Some(previous) = stack.child_by_name("error") {
                stack.remove(&previous);
            }
            stack.add_named(&row, Some("error"));
            stack.set_visible_child_name("error");
            stack.set_visible(true);
        }
        true
    });
    let target = stack.downgrade();
    let loaded = content.downgrade();
    let html = document.clone();
    view.connect_web_process_terminated(move |view, _| {
        if let Some(stack) = target.upgrade() {
            if let Some(content) = loaded.upgrade() {
                content.set_opacity(0.0);
            }
            let weak = view.downgrade();
            let target = stack.downgrade();
            let html = html.clone();
            let row = error(
                "The message renderer stopped",
                std::rc::Rc::new(move || {
                    if let (Some(view), Some(stack)) = (weak.upgrade(), target.upgrade()) {
                        stack.set_visible_child_name("loading");
                        if let Some(html) = html.borrow().as_ref() {
                            view.load_html(html, Some("about:blank"));
                        }
                    }
                }),
            );
            if let Some(previous) = stack.child_by_name("error") {
                stack.remove(&previous);
            }
            stack.add_named(&row, Some("error"));
            stack.set_visible_child_name("error");
            stack.set_visible(true);
        }
    });
    view.connect_map(super::body_layout::observe);
    view.connect_unrealize(|view| view.stop_loading());
    view.connect_decide_policy(|_, decision, kind| {
        if matches!(
            kind,
            webkit6::PolicyDecisionType::NavigationAction
                | webkit6::PolicyDecisionType::NewWindowAction
        ) && let Some(navigation) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
            && let Some(action) = navigation.navigation_action()
            && action.navigation_type() == webkit6::NavigationType::LinkClicked
        {
            if let Some(uri) = action.request().and_then(|request| request.uri())
                && (uri.starts_with("https://")
                    || uri.starts_with("http://")
                    || uri.starts_with("mailto:"))
            {
                super::links::open(uri.as_str());
            }
            decision.ignore();
            return true;
        }
        false
    });
    let (prepared, receiver) = async_channel::bounded(1);
    let source = message.clone();
    std::thread::spawn(move || {
        let html = super::html::document(&source, hide_quotes, media);
        let _ = prepared.send_blocking(html);
    });
    let weak = view.downgrade();
    gtk::glib::spawn_future_local(async move {
        if let Ok(html) = receiver.recv().await
            && let Some(view) = weak.upgrade()
        {
            view.load_html(&html, Some("about:blank"));
            *document.borrow_mut() = Some(html);
        }
    });

    let frame = gtk::Frame::builder()
        .vexpand(false)
        .child(&overlay)
        .hexpand(true)
        .css_classes(["html-viewer-frame"])
        .build();
    (frame.upcast(), view)
}

pub fn enable_media(view: &webkit6::WebView) {
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(view) {
        settings.set_auto_load_images(true);
    }
    // Retry image elements skipped while automatic image loading was disabled.
    view.evaluate_javascript(
        "document.querySelectorAll('img').forEach(image => { for (const name of ['src', 'srcset']) { const source = image.getAttribute(name); if (source && /^(https?:|\\/\\/)/i.test(source.trim())) { image.removeAttribute(name); image.setAttribute(name, source); } } });",
        None,
        None,
        gtk::gio::Cancellable::NONE,
        |result| {
            if let Err(error) = result {
                eprintln!("Could not load message media: {error}");
            }
        },
    );
}

pub fn loading() -> gtk::Widget {
    let row = adw::ActionRow::builder()
        .title("Loading...")
        .height_request(theme::BODY_HEIGHT)
        .build();
    row.add_prefix(
        &gtk::Spinner::builder()
            .spinning(true)
            .width_request(16)
            .height_request(16)
            .valign(gtk::Align::Center)
            .build(),
    );
    row.upcast()
}

pub fn error(message: &str, retry: std::rc::Rc<dyn Fn()>) -> gtk::Widget {
    let row = adw::ActionRow::builder()
        .title("Unable to load message")
        .subtitle(message)
        .use_markup(false)
        .css_classes(["message-load-error"])
        .height_request(theme::BODY_HEIGHT)
        .build();
    let button = gtk::Button::builder()
        .label("Retry")
        .valign(gtk::Align::Center)
        .build();
    button.connect_clicked(move |_| retry());
    row.add_suffix(&button);
    row.upcast()
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn drain(context: &gtk::glib::MainContext, duration: std::time::Duration) {
        let until = std::time::Instant::now() + duration;
        while std::time::Instant::now() < until {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    #[ignore = "Requires a graphical session and WebKit"]
    fn short_message_keeps_its_content_height() {
        gtk::init().unwrap();
        for message in [
            Message {
                body_text: "Hello".into(),
                body_loaded: true,
                ..Default::default()
            },
            Message {
                body_text: "A short message should wrap to the actual card width. ".repeat(4),
                body_loaded: true,
                ..Default::default()
            },
            Message {
                body_html: format!(
                    "<p>Hello</p><blockquote>{}</blockquote>",
                    "<p>Older reply</p>".repeat(100)
                ),
                body_loaded: true,
                ..Default::default()
            },
        ] {
            let (body, view) = view(&message, false, false);
            let peak_request = std::rc::Rc::new(std::cell::Cell::new(theme::BODY_HEIGHT));
            let peak = peak_request.clone();
            view.connect_height_request_notify(move |view| {
                peak.set(peak.get().max(view.height_request()));
            });
            let peak_allocation = std::rc::Rc::new(std::cell::Cell::new(0));
            let peak = peak_allocation.clone();
            body.add_tick_callback(move |body, _| {
                peak.set(peak.get().max(body.height()));
                gtk::glib::ControlFlow::Continue
            });
            let viewer = super::super::column("message-container");
            viewer.append(&body);
            let content = super::super::column("message-viewer-content");
            content.append(&viewer);
            let viewport = gtk::Viewport::builder().child(&content).build();
            let scroll = super::super::scroll(&viewport);
            let window = gtk::Window::builder()
                .default_width(700)
                .default_height(600)
                .child(&scroll)
                .build();
            window.present();
            let context = gtk::glib::MainContext::default();
            drain(&context, std::time::Duration::from_secs(3));
            assert!(view.is_mapped() && !view.is_loading());
            assert_eq!(view.parent().unwrap().opacity(), 1.0);
            assert_eq!(view.height(), view.height_request());
            assert_eq!(peak_request.get(), view.height_request());
            assert_eq!(peak_allocation.get(), body.height());
            assert!(body.height() < 160, "short body height: {}", body.height());
            window.close();
            drain(&context, std::time::Duration::from_millis(250));
        }
    }

    #[test]
    #[ignore = "Requires a graphical session and WebKit"]
    fn full_message_is_measured_after_mapping_and_width_changes() {
        gtk::init().unwrap();
        let html = std::env::var("BREVLADA_HTML_FIXTURE")
            .ok()
            .map(|path| std::fs::read_to_string(path).unwrap())
            .unwrap_or_else(|| format!(
                "<!doctype html><html><head><style>body{{padding:0}} .line{{margin:0;padding:8px}}</style></head><body style='background:lightblue'><div>Header</div></div>{}<p>Bottom</p></body></html>",
                "<p class='line'>A complete line of newsletter content</p>".repeat(80)
            ));
        let message = Message {
            body_html: html,
            body_loaded: true,
            ..Default::default()
        };
        let (body, view) = view(&message, false, false);
        let scroll = gtk::ScrolledWindow::builder().child(&body).build();
        let window = gtk::Window::builder()
            .default_width(700)
            .default_height(400)
            .child(&scroll)
            .build();
        window.present();
        let context = gtk::glib::MainContext::default();
        for width in [700, 450] {
            window.set_default_size(width, 400);
            drain(&context, std::time::Duration::from_secs(3));
            assert!(
                view.height_request() > 1000,
                "measured height: {}",
                view.height_request()
            );
            assert!(
                view.height() >= view.height_request(),
                "allocated {} for measured {}",
                view.height(),
                view.height_request()
            );
            assert!(scroll.vadjustment().upper() > 1000.0);
        }
        window.close();
        drain(&context, std::time::Duration::from_millis(250));
    }
}
