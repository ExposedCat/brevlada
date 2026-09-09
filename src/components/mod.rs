mod body;
mod body_layout;
mod display;
pub mod folders;
mod html;
mod message_row;
pub mod scroll_position;
pub mod shell;
pub mod sidebar;
pub mod states;
pub mod viewer;
use adw::prelude::*;
pub use display::subject;
pub use message_row::{message_row, update as update_message_row};

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
    widget.add_css_class(class);
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

pub fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hexpand(true)
        .vexpand(true)
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
        .resize_start_child(true)
        .shrink_start_child(false)
        .wide_handle(false)
        .build()
}

pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
