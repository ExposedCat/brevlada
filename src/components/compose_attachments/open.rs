use adw::prelude::*;

pub(super) fn attach(card: &super::super::attachments::FileCard, name: &str, part: Option<&str>) {
    let open = card.open_button(name);
    let Some(part) = part else {
        open.set_sensitive(false);
        open.set_tooltip_text(Some("Attachment contents are still loading"));
        return;
    };
    let part = part.to_owned();
    let status = card.status.downgrade();
    let subtitle = card.status.text();
    open.connect_clicked(move |button| {
        button.set_sensitive(false);
        let window = button.root().and_downcast::<gtk::Window>();
        let button = button.downgrade();
        let status = status.clone();
        let subtitle = subtitle.clone();
        let part = part.clone();
        gtk::glib::MainContext::default().spawn_local(async move {
            let result = open_part(part, window.as_ref()).await;
            if let Some(status) = status.upgrade() {
                let text = match result {
                    Ok(()) => subtitle.to_string(),
                    Err(error) => format!("Could not open attachment: {error}"),
                };
                status.set_text(&text);
                status.set_tooltip_text(Some(&text));
            }
            if let Some(button) = button.upgrade() {
                button.set_sensitive(true);
            }
        });
    });
}

async fn open_part(part: String, window: Option<&gtk::Window>) -> Result<(), String> {
    let path = gtk::gio::spawn_blocking(move || crate::backend::attachment_cache::load_part(&part))
        .await
        .map_err(|_| "Could not read attachment".to_owned())?
        .map_err(|error| error.to_string())?;
    let (sender, receiver) = async_channel::bounded(1);
    super::super::links::open_file(&path, window, move |result| {
        let _ = sender.try_send(result);
    });
    receiver.recv().await.map_err(|error| error.to_string())?
}
