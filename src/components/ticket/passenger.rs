use crate::{
    models::ticket::{self, Ticket},
    theme,
};
use adw::prelude::*;

pub(super) fn card(ticket: &Ticket, fallback: Option<&str>, index: usize) -> gtk::Box {
    let tile = super::super::column("ticket-passenger");
    tile.add_css_class("card");
    tile.set_width_request(theme::ATTACHMENT_TILE_WIDTH);
    let content = super::super::column("ticket-tile-content");
    content.set_spacing(theme::SMALL_SPACING);
    let holder = ticket
        .holder
        .as_deref()
        .or(fallback)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Ticket {}", index + 1));
    let title = super::text(&holder, "heading");
    title.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    title.set_width_chars(theme::ATTACHMENT_NAME_MIN_CHARS);
    title.set_max_width_chars(theme::ATTACHMENT_NAME_MAX_CHARS);
    let name = super::super::horizontal("ticket-passenger-name", theme::SMALL_SPACING);
    let icon = gtk::Image::from_icon_name("person-symbolic");
    icon.set_valign(gtk::Align::Start);
    name.append(&icon);
    name.append(&title);
    content.append(&name);
    if let Some(seat) = &ticket.seat {
        let seat = super::text(seat, "");
        seat.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        seat.set_max_width_chars(theme::ATTACHMENT_NAME_MAX_CHARS);
        content.append(&seat);
    }
    let footer = super::super::horizontal("ticket-tile-footer", theme::SMALL_SPACING);
    footer.set_vexpand(true);
    footer.set_valign(gtk::Align::End);
    if let Some(number) = &ticket.number {
        let number = super::text(number, "dim-label");
        number.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        number.set_max_width_chars(theme::ATTACHMENT_NAME_MAX_CHARS);
        footer.append(&number);
    }
    let actions = super::super::horizontal("ticket-copy-actions", 0);
    if let Some(number) = &ticket.number {
        let copy = super::super::button("edit-copy-symbolic", "Copy ticket ID");
        let number = number.clone();
        copy.connect_clicked(move |button| button.clipboard().set_text(&number));
        actions.append(&copy);
    }
    let space = super::super::horizontal("ticket-copy-space", 0);
    let sizes = gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
    sizes.add_widget(&space);
    sizes.add_widget(&actions);
    footer.append(&space);
    content.append(&footer);
    let token = ticket.token.as_deref();
    let url = token.filter(|token| ticket::web_url(token));
    let code = token
        .and_then(|token| token.split_once(':'))
        .filter(|(format, code)| {
            matches!(*format, "qrCode" | "barcode" | "aztecCode") && !code.is_empty()
        })
        .map(|(_, code)| code.to_owned());
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text(if url.is_some() {
            format!("Open ticket for {holder}")
        } else if code.is_some() {
            format!("Copy ticket code for {holder}")
        } else {
            format!("Ticket details for {holder}")
        })
        .hexpand(true)
        .vexpand(true)
        .css_classes(["flat", "ticket-open", "attachment-open"])
        .build();
    if let Some(url) = url {
        let url = url.to_owned();
        button.connect_clicked(move |_| super::super::links::open(&url));
    } else if let Some(code) = code {
        button.connect_clicked(move |button| button.clipboard().set_text(&code));
    } else {
        let details = [
            Some(holder.clone()),
            ticket.seat.clone(),
            ticket.number.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
        button.connect_clicked(move |button| {
            super::super::details::popup(button, "Ticket", &details)
        });
    }
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&button));
    let controls = super::super::horizontal("attachment-overlay-actions", 0);
    controls.set_halign(gtk::Align::End);
    controls.set_valign(gtk::Align::End);
    controls.append(&actions);
    overlay.add_overlay(&controls);
    tile.append(&overlay);
    tile
}
