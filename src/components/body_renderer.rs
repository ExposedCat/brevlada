use std::cell::RefCell;
use webkit6::WebView;

thread_local! {
    static CURRENT: RefCell<Option<WebView>> = const { RefCell::new(None) };
}

pub fn create(settings: &webkit6::Settings, manager: &webkit6::UserContentManager) -> WebView {
    CURRENT.with(|current| {
        let mut builder = WebView::builder()
            .user_content_manager(manager)
            .settings(settings)
            .vexpand(false)
            .height_request(crate::theme::BODY_HEIGHT)
            .hexpand(true);
        if let Some(related) = current.borrow().as_ref() {
            builder = builder.related_view(related);
        }
        let view = builder.build();
        *current.borrow_mut() = Some(view.clone());
        view
    })
}

pub fn clear() {
    CURRENT.with(|current| current.borrow_mut().take());
}
