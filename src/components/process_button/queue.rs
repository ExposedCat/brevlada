use super::*;

pub(super) struct Item {
    pub title: &'static str,
    pub detail: &'static str,
    pub state: State,
}

#[derive(Clone, Copy)]
pub(super) enum State {
    Done,
    Current,
    Waiting,
    Failed { retry_url: Option<&'static str> },
}

pub(super) fn new(items: &[Item], open_link: OpenLink) -> gtk::ScrolledWindow {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list", "process-queue"])
        .build();
    let mut ordered: Vec<_> = items.iter().collect();
    ordered.sort_by_key(|item| !matches!(item.state, State::Failed { .. }));
    for item in ordered {
        let row = adw::ActionRow::builder()
            .title(item.title)
            .subtitle(item.detail)
            .title_lines(1)
            .subtitle_lines(1)
            .use_markup(false)
            .activatable(false)
            .selectable(false)
            .build();
        let icon = gtk::Image::from_icon_name(match item.state {
            State::Done => "object-select-symbolic",
            State::Current => "view-refresh-symbolic",
            State::Waiting => "content-loading-symbolic",
            State::Failed { .. } => "dialog-error-symbolic",
        });
        icon.add_css_class(match item.state {
            State::Done => "success",
            State::Current => "accent",
            State::Waiting => "dim-label",
            State::Failed { .. } => "error",
        });
        row.add_prefix(&icon);
        if matches!(item.state, State::Current) {
            row.add_css_class("process-queue-current");
        }
        if let State::Failed { retry_url } = item.state {
            row.add_css_class("process-queue-failed");
            if let Some(url) = retry_url {
                row.add_suffix(&sample_retry(&row, url, open_link.clone()));
            }
        }
        list.append(&row);
    }
    gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(theme::PROCESS_QUEUE_HEIGHT)
        .build()
}

fn sample_retry(row: &adw::ActionRow, url: &'static str, open_link: OpenLink) -> gtk::Button {
    let button = gtk::Button::with_label("Retry");
    button.set_valign(gtk::Align::Center);
    let target = row.downgrade();
    button.connect_clicked(move |button| {
        button.set_sensitive(false);
        button.set_label("Retrying…");
        let button = button.downgrade();
        let target = target.clone();
        let open = open_link.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            if let Some(row) = target.upgrade() {
                row.set_subtitle("Retry failed · opened unsubscribe page");
            }
            if let Some(button) = button.upgrade() {
                button.set_label("Retry");
                button.set_sensitive(true);
                open(url);
            }
        });
    });
    button
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use std::{cell::RefCell, time::Duration};

    #[test]
    #[ignore = "Requires a private graphical session; uses sample queues and records browser requests"]
    fn failures_lead_the_queue_and_retry_failure_opens_the_unsubscribe_url() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let opened = Rc::new(RefCell::new(Vec::<String>::new()));
        let recorded = opened.clone();
        let samples = super::super::samples::previews();
        let scroll = new(
            &samples[1].items,
            Rc::new(move |url| recorded.borrow_mut().push(url.to_owned())),
        );
        let window = gtk::Window::builder()
            .child(&scroll)
            .default_width(400)
            .default_height(300)
            .build();
        window.present();
        let context = gtk::glib::MainContext::default();
        context.block_on(gtk::glib::timeout_future(Duration::from_millis(100)));
        fn first_row(widget: &gtk::Widget) -> Option<adw::ActionRow> {
            if let Ok(row) = widget.clone().downcast::<adw::ActionRow>() {
                return Some(row);
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Some(row) = first_row(&widget) {
                    return Some(row);
                }
            }
            None
        }
        let first = first_row(scroll.upcast_ref()).unwrap();
        assert_eq!(first.title(), "Dygma");
        fn button(widget: &gtk::Widget) -> Option<gtk::Button> {
            if let Ok(button) = widget.clone().downcast::<gtk::Button>() {
                return Some(button);
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Some(button) = button(&widget) {
                    return Some(button);
                }
            }
            None
        }
        let retry = button(first.upcast_ref()).unwrap();
        assert_eq!(retry.label().as_deref(), Some("Retry"));
        retry.emit_clicked();
        assert!(!retry.is_sensitive());
        assert!(opened.borrow().is_empty());
        context.block_on(gtk::glib::timeout_future(Duration::from_millis(450)));
        assert_eq!(
            *opened.borrow(),
            vec!["https://example.com/unsubscribe".to_owned()]
        );
        assert!(retry.is_sensitive());
        assert_eq!(retry.label().as_deref(), Some("Retry"));
        let sync = new(
            &samples[0].items,
            Rc::new(|_| panic!("Sync must not open unsubscribe URLs")),
        );
        let failed = first_row(sync.upcast_ref()).unwrap();
        assert_eq!(failed.title(), "Archive · Personal account");
        assert!(button(failed.upcast_ref()).is_none());
        window.close();
    }
}
