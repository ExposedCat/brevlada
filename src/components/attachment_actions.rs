use crate::{
    backend::{attachment_cache, attachments},
    models::{Account, Message, attachment},
    theme,
};
use adw::prelude::*;
use std::{cell::Cell, path::PathBuf, rc::Rc};

enum Download {
    Started,
    Finished(Result<PathBuf, String>),
}

struct Actions {
    status: gtk::glib::WeakRef<gtk::Label>,
    open: gtk::glib::WeakRef<gtk::Button>,
    save: gtk::glib::WeakRef<gtk::Button>,
    indicator: gtk::glib::WeakRef<gtk::Stack>,
    spinner: gtk::glib::WeakRef<gtk::Spinner>,
    account: Account,
    folder: String,
    message: Message,
    index: usize,
    busy: Cell<bool>,
    subtitle: String,
}

pub fn attach(
    card: &super::attachments::FileCard,
    account: Option<&Account>,
    folder: &str,
    message: &Message,
    index: usize,
) {
    let name = &message.attachments[index];
    let open = card.open_button(name);
    let save = super::button(
        "folder-download-symbolic",
        &format!("Save {name} elsewhere"),
    );
    let indicator = gtk::Stack::new();
    let spinner = gtk::Spinner::builder()
        .width_request(theme::ATTACHMENT_ICON_SIZE)
        .height_request(theme::ATTACHMENT_ICON_SIZE)
        .build();
    indicator.add_named(
        &gtk::Image::from_icon_name("folder-download-symbolic"),
        Some("icon"),
    );
    indicator.add_named(&spinner, Some("spinner"));
    indicator.set_visible_child_name("icon");
    save.set_child(Some(&indicator));
    card.actions.append(&save);
    open.set_sensitive(account.is_some());
    save.set_sensitive(account.is_some());
    let Some(account) = account else {
        return;
    };
    let actions = Rc::new(Actions {
        status: card.status.downgrade(),
        open: open.downgrade(),
        save: save.downgrade(),
        indicator: indicator.downgrade(),
        spinner: spinner.downgrade(),
        account: account.clone(),
        folder: folder.into(),
        message: message.clone(),
        index,
        busy: Cell::new(false),
        subtitle: card.status.text().to_string(),
    });
    let state = actions.clone();
    open.connect_clicked(move |button| state.start(button, true));
    save.connect_clicked(move |button| actions.start(button, false));
}

impl Actions {
    fn start(self: &Rc<Self>, button: &gtk::Button, open: bool) {
        if self.busy.replace(true) {
            return;
        }
        self.set_enabled(false);
        self.status(&self.subtitle);
        let window = button.root().and_downcast::<gtk::Window>();
        let this = self.clone();
        gtk::glib::MainContext::default().spawn_local(async move {
            let result = this.perform(window.as_ref(), open).await;
            if let Err(error) = result {
                this.status(&format!(
                    "Could not {}: {error}",
                    if open {
                        "open attachment"
                    } else {
                        "save attachment"
                    }
                ));
            }
            this.spinning(false);
            this.busy.set(false);
            this.set_enabled(true);
        });
    }

    async fn perform(&self, window: Option<&gtk::Window>, open: bool) -> Result<(), String> {
        let destination = if open {
            None
        } else {
            let dialog = gtk::FileDialog::builder()
                .title("Save attachment")
                .initial_name(attachment::safe_name(&self.message.attachments[self.index]))
                .build();
            match dialog.save_future(window).await {
                Ok(file) => Some(file),
                Err(error) if super::attachments::cancelled(&error) => return Ok(()),
                Err(error) => return Err(error.to_string()),
            }
        };
        let path = self.cached().await?;
        if let Some(destination) = destination {
            let source = gtk::gio::File::for_path(&path);
            if !source.equal(&destination) {
                self.spinning(true);
                self.status("Saving…");
                let (copy, _) = source.copy_future(
                    &destination,
                    gtk::gio::FileCopyFlags::OVERWRITE,
                    gtk::glib::Priority::DEFAULT,
                );
                copy.await.map_err(|error| error.to_string())?;
            }
            self.status("Saved");
        } else {
            self.spinning(false);
            self.status(&self.subtitle);
            let (sender, receiver) = async_channel::bounded(1);
            super::links::open_file(&path, window, move |result| {
                let _ = sender.try_send(result);
            });
            receiver.recv().await.map_err(|error| error.to_string())??;
        }
        Ok(())
    }

    async fn cached(&self) -> Result<PathBuf, String> {
        let account = self.account.clone();
        let folder = self.folder.clone();
        let message = self.message.clone();
        let index = self.index;
        let (sender, receiver) = async_channel::unbounded();
        std::thread::spawn(move || {
            let result = attachment_cache::load(&account, &folder, &message, index, || {
                let _ = sender.send_blocking(Download::Started);
                attachments::download(&account, &folder, &message, index)
            })
            .map_err(|error| error.to_string());
            let _ = sender.send_blocking(Download::Finished(result));
        });
        loop {
            match receiver.recv().await.map_err(|error| error.to_string())? {
                Download::Started => {
                    self.spinning(true);
                    self.status("Downloading…");
                }
                Download::Finished(result) => return result,
            }
        }
    }

    fn set_enabled(&self, enabled: bool) {
        if let Some(button) = self.open.upgrade() {
            button.set_sensitive(enabled);
        }
        if let Some(button) = self.save.upgrade() {
            button.set_sensitive(enabled);
        }
    }

    fn spinning(&self, spinning: bool) {
        if let (Some(indicator), Some(spinner)) = (self.indicator.upgrade(), self.spinner.upgrade())
        {
            spinner.set_spinning(spinning);
            indicator.set_visible_child_name(if spinning { "spinner" } else { "icon" });
        }
    }

    fn status(&self, text: &str) {
        if let Some(status) = self.status.upgrade() {
            status.set_text(text);
            status.set_tooltip_text(Some(text));
        }
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session for attachment controls"]
    fn clicking_attachment_opens_and_failed_download_restores_both_buttons() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let account = Account {
            path: String::new(),
            email: "attachment-diagnostic@example.com".into(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: 993,
            ssl: false,
            tls: false,
            oauth2: false,
            smtp: None,
        };
        let message = Message {
            message_id: format!(
                "attachment-diagnostic-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            ),
            attachments: vec![format!(
                "{}.txt",
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            )],
            ..Default::default()
        };
        let card = super::super::attachments::FileCard::new("test.txt", Some(3));
        attach(&card, Some(&account), "INBOX", &message, 0);
        fn buttons(widget: &gtk::Widget, result: &mut Vec<gtk::Button>) {
            if let Some(button) = widget.downcast_ref::<gtk::Button>() {
                result.push(button.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                buttons(&widget, result);
                child = widget.next_sibling();
            }
        }
        let mut controls = Vec::new();
        buttons(card.widget.upcast_ref(), &mut controls);
        assert_eq!(controls.len(), 2);
        let (open, save) = (&controls[0], &controls[1]);
        assert!(open.icon_name().is_none());
        assert_eq!(
            open.tooltip_text().as_deref(),
            Some(format!("Open {}", message.attachments[0]).as_str())
        );
        assert!(card.status.is_ancestor(open));
        assert!(!save.is_ancestor(open));
        let indicator = save.child().unwrap().downcast::<gtk::Stack>().unwrap();
        let spinner = indicator
            .child_by_name("spinner")
            .unwrap()
            .downcast::<gtk::Spinner>()
            .unwrap();
        open.emit_clicked();
        assert!(!open.is_sensitive() && !save.is_sensitive());
        let context = gtk::glib::MainContext::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !open.is_sensitive() && std::time::Instant::now() < deadline {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(open.is_sensitive() && save.is_sensitive());
        assert_eq!(indicator.visible_child_name().as_deref(), Some("icon"));
        assert!(!spinner.is_spinning());
        assert!(card.status.text().contains("Could not open attachment"));
        card.status.set_text("3 bytes");
        let path = attachment_cache::path(&account, "INBOX", &message, 0).unwrap();
        assert!(!path.exists());
        let section = super::super::attachments::Section::new();
        section.update(6, Some(18));
        section.append(&card);
        for _ in 0..5 {
            let tile = super::super::attachments::FileCard::new(
                "a long attachment filename that must wrap without widening the pane.txt",
                Some(3),
            );
            attach(&tile, None, "INBOX", &message, 0);
            section.append(&tile);
        }
        assert!(!section.expander.is_expanded());
        let provider = gtk::CssProvider::new();
        provider.load_from_string(theme::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let scroll = super::super::scroll(&section.widget);
        let window = gtk::Window::builder()
            .child(&scroll)
            .default_width(1000)
            .default_height(500)
            .build();
        window.present();
        section.expander.set_expanded(true);
        for width in [1000, 340] {
            window.set_default_size(width, 500);
            let until = std::time::Instant::now() + std::time::Duration::from_millis(250);
            while std::time::Instant::now() < until {
                while context.pending() {
                    context.iteration(false);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let first = section
                .grid
                .child_at_index(0)
                .unwrap()
                .compute_bounds(&section.grid)
                .unwrap();
            let second = section
                .grid
                .child_at_index(1)
                .unwrap()
                .compute_bounds(&section.grid)
                .unwrap();
            if width == 1000 {
                assert_eq!(
                    first.y(),
                    second.y(),
                    "wide pane should have multiple columns"
                );
            } else {
                assert!(second.y() > first.y(), "narrow pane should use one column");
            }
            assert!(section.grid.width() <= width);
            let bounds = open.compute_bounds(&card.widget).unwrap();
            assert_eq!(bounds.x(), 0.0);
            assert_eq!(bounds.y(), 0.0);
            assert_eq!(bounds.width(), card.widget.width() as f32);
            assert_eq!(bounds.height(), card.widget.height() as f32);
            for (x, y) in [
                (bounds.width() / 2.0, 1.0),
                (1.0, bounds.height() / 2.0),
                (bounds.width() - 1.0, bounds.height() / 2.0),
                (bounds.width() / 2.0, bounds.height() - 1.0),
            ] {
                let target = card
                    .widget
                    .pick(x as f64, y as f64, gtk::PickFlags::DEFAULT)
                    .unwrap();
                assert!(target == *open || target.is_ancestor(open));
            }
            let bounds = save.compute_bounds(&card.widget).unwrap();
            let target = card
                .widget
                .pick(
                    (bounds.x() + bounds.width() / 2.0) as f64,
                    (bounds.y() + bounds.height() / 2.0) as f64,
                    gtk::PickFlags::DEFAULT,
                )
                .unwrap();
            assert!(target == *save || target.is_ancestor(save));
            assert!(!target.is_ancestor(open));
        }
        if let Some(path) = std::env::var_os("BREVLADA_ATTACHMENT_PREVIEW") {
            let paintable = gtk::WidgetPaintable::new(Some(&window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
            let texture = window
                .renderer()
                .unwrap()
                .render_texture(&snapshot.to_node().unwrap(), None);
            texture.save_to_png(path).unwrap();
        }
        window.close();
    }
}
