pub mod account_selection;
pub mod account_settings;
pub mod account_switcher;
pub mod action_dialog;
pub mod active_row;
mod attachment_actions;
pub mod attachments;
pub mod autosave;
pub mod avatars;
mod body;
mod body_layout;
pub(crate) mod body_renderer;
pub mod calendar;
pub mod compose;
mod compose_attachments;
pub mod dashboard;
pub mod date_range;
mod details;
pub(crate) mod display;
mod editor_size;
pub mod expansion;
pub mod folders;
pub(crate) mod html;
mod html_editor;
mod html_formatting;
pub mod links;
pub mod mail_shortcuts;
mod menu_item;
mod message_row;
pub mod motion;
mod pane_state;
pub mod parcel;
pub(crate) mod preview;
pub mod receipt;
mod reply_quote;
pub mod scroll_position;
pub mod search_filters;
pub mod sender_menu;
pub mod settings;
mod settings_navigation;
pub mod shell;
pub mod sidebar;
pub mod sort_menu;
pub mod states;
pub mod sync_settings;
pub mod sync_status;
#[allow(dead_code)]
pub mod ticket;
pub mod undo_toast;
pub mod viewer;
pub mod virtual_list;
mod web_fonts;
use adw::prelude::*;
pub use message_row::{message_row, sender_row};

pub fn column(class: &str) -> gtk::Box {
    let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
    widget.add_css_class(class);
    widget
}

pub fn horizontal(class: &str, spacing: i32) -> gtk::Box {
    let widget = gtk::Box::new(gtk::Orientation::Horizontal, spacing);
    widget.add_css_class(class);
    widget
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let widget = gtk::Label::new(Some(text));
    widget.set_xalign(0.0);
    widget.set_halign(gtk::Align::Start);
    widget.set_hexpand(true);
    widget.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if !class.is_empty() {
        widget.add_css_class(class);
    }
    widget
}

pub fn error_label() -> gtk::Label {
    let widget = label("", "error");
    widget.set_halign(gtk::Align::Fill);
    widget.set_ellipsize(gtk::pango::EllipsizeMode::None);
    widget.set_wrap(true);
    widget.set_visible(false);
    widget
}

pub fn selectable_label(text: &str, class: &str) -> gtk::Label {
    let widget = label(text, class);
    widget.set_selectable(true);
    widget
}

pub fn button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .hexpand(false)
        .vexpand(false)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("flat");
    button
}

pub fn action_button(icon: &str, title: &str) -> gtk::Button {
    let content = horizontal("action-button-content", crate::theme::SMALL_SPACING);
    content.append(&gtk::Image::from_icon_name(icon));
    let title = label(title, "");
    title.set_hexpand(false);
    title.set_ellipsize(gtk::pango::EllipsizeMode::None);
    content.append(&title);
    gtk::Button::builder()
        .child(&content)
        .halign(gtk::Align::Start)
        .css_classes(["pill"])
        .build()
}

pub fn card_actions() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .halign(gtk::Align::Start)
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(crate::theme::SMALL_SPACING as u32)
        .row_spacing(crate::theme::SMALL_SPACING as u32)
        .min_children_per_line(1)
        .max_children_per_line(4)
        .css_classes(["card-actions"])
        .build()
}

pub fn append_action(actions: &gtk::FlowBox, button: &gtk::Button) {
    let child = gtk::FlowBoxChild::builder()
        .child(button)
        .focusable(false)
        .build();
    actions.append(&child);
}

pub fn card_title(icon: &str, title: &str) -> gtk::Box {
    let row = horizontal("card-title", crate::theme::SMALL_SPACING);
    row.add_css_class("title-2");
    let icon = gtk::Image::from_icon_name(icon);
    icon.set_valign(gtk::Align::Start);
    row.append(&icon);
    let title = selectable_label(title, "");
    title.set_halign(gtk::Align::Fill);
    title.set_wrap(true);
    title.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    title.set_ellipsize(gtk::pango::EllipsizeMode::None);
    row.append(&title);
    row
}

pub fn card_detail(icon: &str, value: &str) -> gtk::Box {
    let row = horizontal("card-detail", crate::theme::SMALL_SPACING);
    let icon = gtk::Image::from_icon_name(icon);
    icon.set_valign(gtk::Align::Start);
    row.append(&icon);
    let value = selectable_label(value, "");
    value.set_halign(gtk::Align::Fill);
    value.set_wrap(true);
    value.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value.set_ellipsize(gtk::pango::EllipsizeMode::None);
    row.append(&value);
    row
}

pub fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hexpand(true)
        .vexpand(true)
        .kinetic_scrolling(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build()
}

pub fn pane(
    left: &impl IsA<gtk::Widget>,
    right: &impl IsA<gtk::Widget>,
    position: i32,
) -> gtk::Paned {
    gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(left)
        .end_child(right)
        .position(position)
        .resize_start_child(false)
        .resize_end_child(true)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .wide_handle(false)
        .build()
}

pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
