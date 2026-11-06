use super::*;
use crate::models::dashboard::{Occurrence, Source};

pub(super) fn popover(occurrences: &[Occurrence]) -> (gtk::MenuButton, Vec<(gtk::Button, Source)>) {
    let content = column("dashboard-series");
    content.set_spacing(theme::SMALL_SPACING);
    content.set_size_request(theme::CARD_COLUMN_WIDTH, -1);
    content.append(&label("Series in your mail", "heading"));
    let dates = column("dashboard-series-dates");
    dates.set_spacing(theme::SMALL_SPACING);
    let popover = gtk::Popover::builder().child(&content).build();
    let mut buttons = Vec::new();
    for occurrence in occurrences {
        let mut date = occurrence.event.date_label();
        if occurrence.event.status.as_deref().is_some_and(|status| {
            matches!(
                status.rsplit('/').next(),
                Some("CANCELLED" | "EventCancelled")
            )
        }) {
            date.push_str(" · Cancelled");
        }
        let open = super::super::action_button("mail-unread-symbolic", &date);
        open.remove_css_class("pill");
        open.add_css_class("flat");
        open.set_tooltip_text(Some("Open this occurrence’s email"));
        let popup = popover.downgrade();
        open.connect_clicked(move |_| {
            if let Some(popover) = popup.upgrade() {
                popover.popdown();
            }
        });
        dates.append(&open);
        buttons.push((open, occurrence.source.clone()));
    }
    let scroll = gtk::ScrolledWindow::builder()
        .child(&dates)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(theme::DASHBOARD_SERIES_HEIGHT)
        .min_content_width(theme::CARD_COLUMN_WIDTH)
        .build();
    content.append(&scroll);
    let button = gtk::MenuButton::builder()
        .icon_name("media-playlist-repeat-symbolic")
        .tooltip_text("Show events in this series")
        .popover(&popover)
        .css_classes(["flat"])
        .valign(gtk::Align::Start)
        .build();
    (button, buttons)
}
