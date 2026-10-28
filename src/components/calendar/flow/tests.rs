use super::*;
use crate::models::calendar::Event;
use std::time::Duration;

#[test]
#[ignore = "Requires a private graphical session; presents fixture event cards without mail workers"]
fn wraps_long_events_with_equal_row_heights_and_no_focusable_rows() {
    gtk::init().unwrap();
    adw::init().unwrap();
    let display = gtk::gdk::Display::default().unwrap();
    let css = gtk::CssProvider::new();
    css.load_from_string(theme::CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &css,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let events: Vec<gtk::Widget> = (0..3)
        .map(|index| {
            super::super::card(
                &Event {
                    name: "Appointment".into(),
                    start: "2027-01-25T09:30:00+01:00".into(),
                    end: Some("2027-01-25T10:00:00+01:00".into()),
                    location: Some("Medical Centre, Na Dlouhém lánu 563/11 (entrance from the side street), Prague 6, 160 00, CZ".into()),
                    organizer: (index == 0).then(|| "Medical Centre: Doctor".into()),
                    ..Default::default()
                },
                "me@example.com",
                false,
                |_| {},
            )
            .upcast()
        })
        .collect();
    let flow = Flow::new(&events);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.append(&flow);
    let footer = gtk::Label::new(Some("Message content begins here"));
    content.append(&footer);
    let scroll = crate::components::scroll(&content);
    let window = gtk::Window::builder()
        .title("Event card layout diagnostic")
        .default_width(900)
        .default_height(800)
        .child(&scroll)
        .build();
    window.present();
    let context = glib::MainContext::default();
    for width in [900, 360, 1500] {
        window.set_default_size(width, 800);
        context.block_on(glib::timeout_future(Duration::from_millis(150)));
        let bounds: Vec<_> = events
            .iter()
            .map(|event| event.compute_bounds(&flow).unwrap())
            .collect();
        if width == 360 {
            assert!(bounds[1].y() > bounds[0].y());
        } else {
            assert_eq!(bounds[0].y(), bounds[1].y());
            assert!(bounds[2].y() > bounds[1].y());
            assert_eq!(bounds[0].height(), bounds[1].height());
        }
        let columns = if width == 360 { 1 } else { 2 };
        for (row, bounds) in events.chunks(columns).zip(bounds.chunks(columns)) {
            let row_height = row
                .iter()
                .zip(bounds)
                .map(|(event, bounds)| {
                    event
                        .measure(gtk::Orientation::Vertical, bounds.width() as i32)
                        .1
                })
                .max()
                .unwrap();
            for bounds in bounds {
                // Each row fits its tallest card, including the existing CSS margin.
                assert!((row_height - bounds.height() as i32).abs() <= theme::SMALL_SPACING);
            }
        }
        let bottom = bounds
            .iter()
            .map(|bounds| bounds.y() + bounds.height())
            .fold(0.0, f32::max);
        let gap = footer.compute_bounds(&flow).unwrap().y() - bottom;
        assert!(gap <= theme::SMALL_SPACING as f32);
    }
    assert!(!flow.is_focusable());
    GtkWindowExt::set_focus(&window, gtk::Widget::NONE);
    for _ in &events {
        assert!(window.child_focus(gtk::DirectionType::TabForward));
        assert!(GtkWindowExt::focus(&window).unwrap().is::<gtk::Button>());
    }
    if std::env::var_os("BREVLADA_DIAGNOSTIC_HOLD").is_some() {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
        GtkWindowExt::set_focus(&window, gtk::Widget::NONE);
        context.block_on(glib::timeout_future(Duration::from_secs(60)));
    }
    window.destroy();
}
