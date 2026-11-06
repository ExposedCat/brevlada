use super::*;
use crate::{models::parcel::Point, theme};

pub(super) fn view(parcel: &Parcel) -> gtk::Box {
    let timeline = super::super::horizontal("parcel-timeline", theme::SMALL_SPACING);
    for (index, point) in parcel.timeline().iter().enumerate() {
        if index > 0 {
            let line = gtk::Separator::new(gtk::Orientation::Horizontal);
            line.set_hexpand(true);
            line.set_valign(gtk::Align::Center);
            line.add_css_class("parcel-line");
            timeline.append(&line);
        }
        timeline.append(&button(point));
    }
    timeline
}

fn text(value: &str, class: &str) -> gtk::Label {
    let text = super::super::label(value, class);
    text.set_wrap(true);
    text.set_wrap_mode(gtk::pango::WrapMode::Word);
    text.set_ellipsize(gtk::pango::EllipsizeMode::None);
    text
}

fn button(point: &Point) -> gtk::Button {
    let content = super::super::horizontal("parcel-step-content", theme::SMALL_SPACING);
    let icon = gtk::Image::from_icon_name("truck-symbolic");
    icon.set_valign(gtk::Align::Center);
    icon.add_css_class(if point.confirmed {
        "parcel-step-done"
    } else {
        "parcel-step-pending"
    });
    content.append(&icon);
    let name = point.step.name.as_deref().unwrap_or("Delivery update");
    content.append(&text(name, "parcel-step-name"));
    let date = point
        .step
        .date
        .as_deref()
        .map(super::date)
        .unwrap_or_else(|| "Unknown date".into());
    let button = gtk::Button::builder()
        .child(&content)
        .hexpand(false)
        .valign(gtk::Align::Center)
        .css_classes(["flat", "parcel-step"])
        .build();
    if !point.confirmed {
        button.add_css_class("parcel-step-inferred");
    }
    let details = [Some(date), point.step.location.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    let name = name.to_owned();
    button.connect_clicked(move |button| {
        super::super::details::popup(button, &name, &details);
    });
    button
}
