use adw::prelude::*;

pub fn confirm(
    parent: Option<&gtk::Window>,
    question: &str,
    verb: &str,
    destructive: bool,
    confirmed: impl Fn() + 'static,
) -> adw::MessageDialog {
    let dialog = adw::MessageDialog::new(parent, Some(question), None);
    dialog.add_responses(&[("cancel", "Cancel"), ("confirm", verb)]);
    dialog.set_response_appearance(
        "confirm",
        if destructive {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "confirm" {
            confirmed();
        }
    });
    dialog.present();
    dialog
}

pub fn move_to(
    parent: Option<&gtk::Window>,
    folders: Vec<String>,
    chosen: impl Fn(&str) + 'static,
) -> adw::MessageDialog {
    let dialog = adw::MessageDialog::new(
        parent,
        Some("Move to…"),
        Some("Choose a destination folder."),
    );
    let names: Vec<_> = folders.iter().map(String::as_str).collect();
    let destination = gtk::DropDown::from_strings(&names);
    destination.set_enable_search(true);
    destination.update_property(&[gtk::accessible::Property::Label("Destination folder")]);
    dialog.set_extra_child(Some(&destination));
    dialog.add_responses(&[("cancel", "Cancel"), ("move", "Move")]);
    dialog.set_response_appearance("move", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("move"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "move"
            && let Some(folder) = folders.get(destination.selected() as usize)
        {
            chosen(folder);
        }
    });
    dialog.present();
    dialog
}
