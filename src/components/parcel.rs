use crate::models::parcel::{Parcel, Step};
use adw::prelude::*;

mod details;

fn label(value: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(value));
    label.set_xalign(0.0);
    label.set_wrap(true);
    if !class.is_empty() {
        label.add_css_class(class);
    }
    label
}

fn date(value: &str) -> String {
    details::date(value)
}

fn step_button(step: &Step, complete: bool, single: bool) -> gtk::Button {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let icon = gtk::Image::from_icon_name("truck-symbolic");
    icon.set_valign(gtk::Align::Center);
    if !single {
        icon.add_css_class(if complete {
            "parcel-step-done"
        } else {
            "parcel-step-pending"
        });
    }
    content.append(&icon);
    if let Some(name) = &step.name {
        let text = label(
            name,
            if single {
                "parcel-single-step-name"
            } else {
                "parcel-step-name"
            },
        );
        content.append(&text);
    }
    let button = gtk::Button::builder()
        .child(&content)
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("flat");
    button.add_css_class(if single {
        "parcel-single-step"
    } else {
        "parcel-step"
    });
    let step = step.clone();
    button.connect_clicked(move |button| {
        let details = [
            step.date.as_ref().map(|date_value| date(date_value)),
            step.location.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
        super::details::popup(
            button,
            step.name.as_deref().unwrap_or("Delivery update"),
            &details,
        );
    });
    button
}

fn order_popup(anchor: &gtk::Button, parcel: &Parcel, timestamp: i64) -> adw::Window {
    let window = adw::Window::builder()
        .title("Order Details")
        .default_width(460)
        .default_height(560)
        .modal(true)
        .build();
    if let Some(parent) = anchor.root().and_downcast::<gtk::Window>() {
        window.set_transient_for(Some(&parent));
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);
    let mut sections = details::sections(&parcel.details);
    if let Some(date) = chrono::DateTime::from_timestamp(timestamp, 0) {
        let row = details::Detail {
            title: "Email date".into(),
            value: date
                .with_timezone(&chrono::Local)
                .format("%A, %-d %B %Y at %H:%M")
                .to_string(),
            url: None,
        };
        if let Some(section) = sections
            .iter_mut()
            .find(|section| section.title == "Delivery")
        {
            section.rows.insert(0, row);
        } else {
            sections.insert(
                0,
                details::Section {
                    title: "Delivery".into(),
                    rows: vec![row],
                },
            );
        }
    }
    let has_order_link = sections
        .iter()
        .flat_map(|section| &section.rows)
        .any(|row| row.url.is_some() && row.url.as_ref() == parcel.order_url.as_ref());
    for section in sections {
        let group = adw::PreferencesGroup::builder()
            .title(&section.title)
            .build();
        for detail in section.rows {
            let row = adw::ActionRow::builder()
                .title(&detail.title)
                .use_markup(false)
                .build();
            if let Some(url) = detail.url {
                let link = gtk::Button::with_label(&detail.value);
                link.add_css_class("flat");
                link.set_valign(gtk::Align::Center);
                link.connect_clicked(move |_| super::links::open(&url));
                row.add_suffix(&link);
                row.set_activatable_widget(Some(&link));
            } else {
                row.set_subtitle(&detail.value);
                row.set_subtitle_selectable(true);
            }
            group.add(&row);
        }
        content.append(&group);
    }
    if content.first_child().is_none() {
        content.append(&label(
            "No additional order details available.",
            "dim-label",
        ));
    }
    let scroll = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let layout = gtk::Box::new(gtk::Orientation::Vertical, 12);
    layout.append(&scroll);
    if !has_order_link
        && let Some(url) = &parcel.order_url
        && (url.starts_with("https://") || url.starts_with("http://"))
    {
        let link = super::action_button("web-browser-symbolic", "View order");
        let url = url.clone();
        link.connect_clicked(move |_| super::links::open(&url));
        link.set_margin_start(18);
        link.set_margin_end(18);
        link.set_margin_bottom(18);
        layout.append(&link);
    }
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Order Details", "")));
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&layout));
    window.set_content(Some(&toolbar));
    window.present();
    window
}

pub fn card(parcel: &Parcel, timestamp: i64) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 12);
    card.add_css_class("parcel-card");
    if let Some(item) = &parcel.item {
        card.append(&label(item, "title-2"));
    }
    if let Some(until) = &parcel.arrival_until {
        let arrival = if let Some(from) = &parcel.arrival_from {
            format!("Arriving on {} – {}", date(from), date(until))
        } else {
            format!("Arriving by {}", date(until))
        };
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&gtk::Image::from_icon_name("truck-symbolic"));
        row.append(&label(&arrival, "title-4"));
        card.append(&row);
    }
    let delivery = [
        parcel
            .carrier
            .as_deref()
            .map(str::trim)
            .filter(|carrier| !carrier.is_empty())
            .map(|carrier| format!("by {carrier}")),
        details::address(&parcel.details["deliveryAddress"]),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    if !delivery.is_empty() {
        card.append(&label(&delivery, "dim-label"));
    }
    if parcel.steps.is_empty()
        && let Some(status) = parcel
            .details
            .get("deliveryStatus")
            .and_then(|value| value.as_str())
        && !status.contains("://")
    {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.add_css_class("parcel-single-step");
        row.append(&gtk::Image::from_icon_name("truck-symbolic"));
        row.append(&label(status, "parcel-single-step-name"));
        card.append(&row);
    }
    let mut steps = parcel.steps.clone();
    let expected_step = !steps.is_empty()
        && parcel.arrival_until.is_some()
        && !steps.iter().any(|step| {
            step.name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case("delivered"))
        });
    if expected_step {
        steps.push(Step {
            name: Some("Expected delivery".into()),
            date: parcel.arrival_until.clone(),
            location: None,
        });
    }
    if !steps.is_empty() {
        let timeline = gtk::Box::new(gtk::Orientation::Horizontal, crate::theme::SPACING);
        timeline.add_css_class("parcel-timeline");
        let single = steps.len() == 1;
        for (index, step) in steps.iter().enumerate() {
            if index > 0 {
                let line = gtk::Separator::new(gtk::Orientation::Horizontal);
                line.set_hexpand(true);
                line.set_valign(gtk::Align::Center);
                line.add_css_class("parcel-line");
                timeline.append(&line);
            }
            let pending = expected_step && index == steps.len() - 1;
            timeline.append(&step_button(step, !pending, single));
        }
        let scroll = gtk::ScrolledWindow::builder()
            .child(&timeline)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .build();
        card.append(&scroll);
    }
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    if let Some(url) = &parcel.tracking_url
        && (url.starts_with("https://") || url.starts_with("http://"))
    {
        let track = super::action_button("web-browser-symbolic", "Track Package");
        let url = url.clone();
        track.connect_clicked(move |_| super::links::open(&url));
        actions.append(&track);
    }
    let order = super::action_button("document-properties-symbolic", "Order Details");
    let order_parcel = parcel.clone();
    let order_number = parcel.order_number.clone();
    order.connect_clicked(move |button| {
        order_popup(button, &order_parcel, timestamp);
    });
    actions.append(&order);
    if parcel.order_url.is_none()
        && let Some(number) = order_number
    {
        let copy = gtk::Button::with_label(&format!("Order {number}"));
        copy.add_css_class("flat");
        copy.set_tooltip_text(Some("Copy order number"));
        copy.connect_clicked(move |button| button.clipboard().set_text(&number));
        actions.append(&copy);
    }
    card.append(&actions);
    card
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn descendants(widget: &gtk::Widget, output: &mut Vec<gtk::Widget>) {
        output.push(widget.clone());
        let mut child = widget.first_child();
        while let Some(next) = child {
            descendants(&next, output);
            child = next.next_sibling();
        }
    }

    fn parcel(steps: Vec<Step>) -> Parcel {
        Parcel {
            item: Some("Test package".into()),
            arrival_from: None,
            arrival_until: None,
            carrier: None,
            tracking_url: None,
            order_url: None,
            order_number: None,
            steps,
            details: serde_json::json!({"deliveryStatus":"In transit"}),
        }
    }

    #[test]
    #[ignore = "Requires a graphical session to inspect the order dialog"]
    fn order_dialog_uses_grouped_selectable_details_and_named_links() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let mut parcel = parcel(vec![]);
        parcel.item = Some("USB-C charging hub".into());
        parcel.order_url = Some("https://example.com/orders/123".into());
        parcel.details = serde_json::json!({
            "@type": "ParcelDelivery",
            "carrier": {"name": "Parcel Co"},
            "trackingNumber": "XX12345678",
            "deliveryStatus": "In transit",
            "expectedArrivalUntil": "2026-10-03",
            "trackingUrl": "https://example.com/track?token=long-opaque-token",
            "partOfOrder": {
                "orderNumber": "123",
                "orderDate": "2026-09-29T12:30:00Z",
                "url": "https://example.com/orders/123"
            },
            "deliveryAddress": {"streetAddress": "1 Main Street\nApartment 2", "addressLocality": "London"}
        });
        let anchor = gtk::Button::new();
        let parent = gtk::Window::builder().child(&anchor).build();
        let timestamp = chrono::DateTime::parse_from_rfc3339("2026-10-01T12:30:00Z")
            .unwrap()
            .timestamp();
        let window = order_popup(&anchor, &parcel, timestamp);
        let context = gtk::glib::MainContext::default();
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(window.is_modal());
        assert_eq!(window.transient_for(), Some(parent.clone()));
        let mut widgets = Vec::new();
        descendants(window.upcast_ref(), &mut widgets);
        assert!(
            widgets
                .iter()
                .filter_map(|widget| widget.clone().downcast::<adw::WindowTitle>().ok())
                .all(|title| title.subtitle().is_empty())
        );
        let rows: Vec<_> = widgets
            .iter()
            .filter_map(|widget| widget.clone().downcast::<adw::ActionRow>().ok())
            .collect();
        assert!(rows.iter().any(|row| {
            row.title() == "Email date"
                && row.subtitle().as_deref()
                    == Some(
                        &chrono::DateTime::from_timestamp(timestamp, 0)
                            .unwrap()
                            .with_timezone(&chrono::Local)
                            .format("%A, %-d %B %Y at %H:%M")
                            .to_string(),
                    )
                && row.is_subtitle_selectable()
        }));
        assert!(rows.iter().any(|row| {
            row.title() == "Order number"
                && row.subtitle().as_deref() == Some("123")
                && row.is_subtitle_selectable()
        }));
        assert!(rows.iter().all(|row| {
            !row.uses_markup() && !row.subtitle().is_some_and(|value| value.contains("://"))
        }));
        let buttons: Vec<_> = widgets
            .iter()
            .filter_map(|widget| widget.clone().downcast::<gtk::Button>().ok())
            .collect();
        assert!(
            buttons
                .iter()
                .any(|button| button.label().as_deref() == Some("Track package"))
        );
        assert_eq!(
            buttons
                .iter()
                .filter(|button| button.label().as_deref() == Some("View order"))
                .count(),
            1
        );
        if let Some(path) = std::env::var_os("BREVLADA_ORDER_PREVIEW") {
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            let node = snapshot.to_node().unwrap();
            let texture = window.renderer().unwrap().render_texture(&node, None);
            texture.save_to_png(path).unwrap();
        }
        window.close();
        parent.close();
    }

    #[test]
    #[ignore = "Requires a graphical session to measure GTK widgets"]
    fn timeline_lines_are_thin() {
        gtk::init().unwrap();
        adw::init().unwrap();
        gtk::gio::resources_register_include!("brevlada.gresource").unwrap();
        gtk::IconTheme::for_display(&gtk::gdk::Display::default().unwrap())
            .add_resource_path("/org/gtk/example/icons");
        let css = gtk::CssProvider::new();
        css.load_from_string(crate::theme::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let card = card(
            &parcel(vec![
                Step {
                    name: Some("Shipped".into()),
                    date: None,
                    location: None,
                },
                Step {
                    name: Some("Delivered".into()),
                    date: None,
                    location: None,
                },
            ]),
            0,
        );
        let window = gtk::Window::builder()
            .default_width(500)
            .child(&card)
            .build();
        window.present();
        let context = gtk::glib::MainContext::default();
        for _ in 0..10 {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut widgets = Vec::new();
        descendants(card.upcast_ref(), &mut widgets);
        let line = widgets
            .iter()
            .find(|widget| widget.has_css_class("parcel-line"))
            .unwrap();
        assert!(
            (1..=3).contains(&line.height()),
            "timeline line is {}px high",
            line.height()
        );
        if let Some(path) = std::env::var_os("BREVLADA_PARCEL_PREVIEW") {
            let paintable = gtk::WidgetPaintable::new(Some(&card));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, card.width() as f64, card.height() as f64);
            let texture = window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None);
            texture.save_to_png(path).unwrap();
        }
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session to inspect GTK widgets"]
    fn single_status_and_step_use_truck_and_normal_text() {
        gtk::init().unwrap();
        adw::init().unwrap();
        for steps in [
            vec![],
            vec![Step {
                name: Some("In transit".into()),
                date: None,
                location: None,
            }],
        ] {
            let card = card(&parcel(steps), 0);
            let mut widgets = Vec::new();
            descendants(card.upcast_ref(), &mut widgets);
            assert!(
                widgets
                    .iter()
                    .any(|widget| widget.has_css_class("parcel-single-step-name"))
            );
            assert!(widgets.iter().any(|widget| {
                widget
                    .clone()
                    .downcast::<gtk::Image>()
                    .is_ok_and(|image| image.icon_name().as_deref() == Some("truck-symbolic"))
            }));
            assert!(
                !widgets
                    .iter()
                    .any(|widget| widget.has_css_class("dim-label"))
            );
        }
    }
}
