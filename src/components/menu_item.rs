use adw::prelude::*;

pub fn action(title: &str, icon: &str, shortcut: Option<&str>, name: &str) -> gtk::Button {
    let content = super::horizontal("mail-action-content", crate::theme::SMALL_SPACING);
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&super::label(title, "mail-action-title"));
    if let Some(shortcut) = shortcut {
        let label = super::label(shortcut, "mail-action-shortcut");
        label.set_hexpand(false);
        label.set_halign(gtk::Align::End);
        content.append(&label);
    }
    let button = gtk::Button::builder()
        .child(&content)
        .action_name(name)
        .accessible_role(gtk::AccessibleRole::MenuItem)
        .css_classes(["flat", "mail-action"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(title)]);
    button
}
