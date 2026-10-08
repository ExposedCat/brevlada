use crate::{
    models::ticket::{self, Reservation},
    theme,
};
use adw::prelude::*;

fn stop(title: &str, date: &Option<String>, location: &Option<String>) -> gtk::Button {
    let summary = [
        date.as_deref().and_then(ticket::time_label),
        location.clone(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let label = super::text(&summary, "");
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_width_chars(theme::ATTACHMENT_NAME_MIN_CHARS);
    let button = gtk::Button::builder()
        .child(&label)
        .hexpand(true)
        .tooltip_text(format!("Show {title} location"))
        .css_classes(["flat", "ticket-stop"])
        .build();
    let details = [date.as_deref().map(ticket::date_label), location.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    let title = title.to_owned();
    button.connect_clicked(move |button| super::super::details::popup(button, &title, &details));
    button
}

pub(super) fn view(reservation: &Reservation) -> gtk::Box {
    let timeline = super::super::column("ticket-timeline");
    timeline.set_spacing(theme::SPACING);
    let dates: Vec<_> = [reservation.start.as_deref(), reservation.end.as_deref()]
        .into_iter()
        .flatten()
        .map(ticket::day_label)
        .collect();
    if let Some(start) = dates.first() {
        let range = match dates.last().filter(|end| *end != start) {
            Some(end) => format!("{start} – {end}"),
            None => start.clone(),
        };
        timeline.append(&super::text(&range, "dim-label"));
    }
    let grid = gtk::Grid::builder()
        .column_spacing(theme::SMALL_SPACING)
        .css_classes(["ticket-destinations"])
        .build();
    let mut stops = Vec::new();
    if reservation.start.is_some() || reservation.departure.is_some() {
        stops.push((
            "user-home-symbolic",
            stop("Departure", &reservation.start, &reservation.departure),
        ));
    }
    if reservation.end.is_some() || reservation.arrival.is_some() {
        stops.push((
            "location-services-active-symbolic",
            stop("Arrival", &reservation.end, &reservation.arrival),
        ));
    }
    for (index, (icon, stop)) in stops.iter().enumerate() {
        let row = index as i32 * 2;
        if index > 0 {
            let line = gtk::Separator::new(gtk::Orientation::Vertical);
            line.set_halign(gtk::Align::Center);
            line.add_css_class("ticket-stop-line");
            grid.attach(&line, 0, row - 1, 1, 1);
        }
        grid.attach(&gtk::Image::from_icon_name(icon), 0, row, 1, 1);
        grid.attach(stop, 1, row, 1, 1);
    }
    timeline.append(&grid);
    timeline
}
