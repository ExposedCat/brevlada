use crate::{models::Message, theme};
use adw::prelude::*;
use webkit6::prelude::*;

pub fn view(message: &Message) -> gtk::Widget {
    let settings = webkit6::Settings::builder()
        .enable_javascript(true)
        .enable_javascript_markup(false)
        .enable_html5_local_storage(false)
        .auto_load_images(false)
        .build();
    let manager = webkit6::UserContentManager::new();
    let view = webkit6::WebView::builder()
        .user_content_manager(&manager)
        .vexpand(false)
        .settings(&settings)
        .height_request(theme::BODY_HEIGHT)
        .hexpand(true)
        .build();
    super::body_layout::connect(&view, &manager);
    let stack = gtk::Stack::builder()
        .vexpand(false)
        .height_request(theme::BODY_HEIGHT)
        .vhomogeneous(false)
        .build();
    stack.add_named(&loading(), Some("loading"));
    stack.add_named(&view, Some("content"));
    stack.set_visible_child_name("loading");
    let target = stack.downgrade();
    view.connect_load_changed(move |view, event| {
        if event == webkit6::LoadEvent::Finished
            && let Some(stack) = target.upgrade()
            && stack.visible_child_name().as_deref() != Some("error")
        {
            stack.set_visible_child_name("content");
            super::body_layout::observe(view);
        }
    });
    let document = std::rc::Rc::new(super::html::document(message));
    let target = stack.downgrade();
    let html = document.clone();
    view.connect_load_failed(move |view, _, _, failure| {
        if let Some(stack) = target.upgrade() {
            let weak = view.downgrade();
            let target = stack.downgrade();
            let html = html.clone();
            let row = error(
                &failure.to_string(),
                std::rc::Rc::new(move || {
                    if let (Some(view), Some(stack)) = (weak.upgrade(), target.upgrade()) {
                        stack.set_visible_child_name("loading");
                        view.load_html(&html, Some("about:blank"));
                    }
                }),
            );
            if let Some(previous) = stack.child_by_name("error") {
                stack.remove(&previous);
            }
            stack.add_named(&row, Some("error"));
            stack.set_visible_child_name("error");
        }
        true
    });
    let target = stack.downgrade();
    let html = document.clone();
    view.connect_web_process_terminated(move |view, _| {
        if let Some(stack) = target.upgrade() {
            let weak = view.downgrade();
            let target = stack.downgrade();
            let html = html.clone();
            let row = error(
                "The message renderer stopped",
                std::rc::Rc::new(move || {
                    if let (Some(view), Some(stack)) = (weak.upgrade(), target.upgrade()) {
                        stack.set_visible_child_name("loading");
                        view.load_html(&html, Some("about:blank"));
                    }
                }),
            );
            if let Some(previous) = stack.child_by_name("error") {
                stack.remove(&previous);
            }
            stack.add_named(&row, Some("error"));
            stack.set_visible_child_name("error");
        }
    });
    view.connect_unrealize(|view| view.stop_loading());
    view.load_html(&document, Some("about:blank"));
    view.connect_decide_policy(|_, decision, kind| {
        if kind == webkit6::PolicyDecisionType::NavigationAction
            && let Some(navigation) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
            && let Some(action) = navigation.navigation_action()
            && action.navigation_type() == webkit6::NavigationType::LinkClicked
        {
            if let Some(uri) = action.request().and_then(|request| request.uri())
                && (uri.starts_with("https://")
                    || uri.starts_with("http://")
                    || uri.starts_with("mailto:"))
            {
                gtk::gio::AppInfo::launch_default_for_uri_async(
                    &uri,
                    None::<&gtk::gio::AppLaunchContext>,
                    gtk::gio::Cancellable::NONE,
                    |_| {},
                );
            }
            decision.ignore();
            return true;
        }
        false
    });

    let frame = gtk::Frame::builder()
        .vexpand(false)
        .child(&stack)
        .hexpand(true)
        .css_classes(["html-viewer-frame"])
        .build();
    frame.upcast()
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
