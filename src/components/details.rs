use adw::prelude::*;

pub fn popup(anchor: &impl IsA<gtk::Widget>, title: &str, value: &str) {
    let popover = gtk::Popover::new();
    popover.set_parent(anchor);
    let content = super::column("detail-popup");
    content.set_spacing(crate::theme::SMALL_SPACING);
    content.append(&super::label(title, "title-4"));
    let label = super::label(value, "");
    label.set_wrap(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::None);
    content.append(&label);
    popover.set_child(Some(&content));
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}
