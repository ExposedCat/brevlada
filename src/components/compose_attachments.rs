use crate::{backend::attachments, models::Draft, theme};
use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Item {
    name: String,
    size: Option<u64>,
    part: Option<String>,
    source: Option<usize>,
}

type ChangedAction = RefCell<Option<Box<dyn Fn()>>>;

type LoadingAction = RefCell<Option<Box<dyn Fn(bool)>>>;

pub struct Attachments {
    pub widget: gtk::Box,
    grid: gtk::FlowBox,
    pub toolbar: gtk::Box,
    button: gtk::Button,
    status: gtk::Label,
    items: RefCell<Vec<Item>>,
    cards: RefCell<Vec<super::attachments::FileCard>>,
    removed: RefCell<Vec<usize>>,
    loading: Cell<bool>,
    revision: Cell<u64>,
    on_loading: LoadingAction,
    on_changed: ChangedAction,
    loaded: Cell<bool>,
    inline: RefCell<Vec<String>>,
}

impl Attachments {
    pub fn new() -> Rc<Self> {
        let widget = super::column("attachment-section");
        let grid = super::attachments::grid();
        widget.append(&grid);
        let toolbar = super::horizontal("attachment-toolbar", theme::SMALL_SPACING);
        let content = super::horizontal("attachment-button-content", theme::SMALL_SPACING);
        content.append(&gtk::Image::from_icon_name("mail-attachment-symbolic"));
        content.append(&gtk::Label::new(Some("Attach files")));
        let button = gtk::Button::builder()
            .child(&content)
            .tooltip_text("Attach files (up to 20 MB total)")
            .build();
        let status = super::label("", "dim-label");
        toolbar.append(&button);
        toolbar.append(&status);
        let this = Rc::new(Self {
            widget,
            grid,
            toolbar,
            button,
            status,
            items: RefCell::default(),
            cards: RefCell::default(),
            removed: RefCell::default(),
            loading: Cell::new(false),
            revision: Cell::new(0),
            on_loading: RefCell::default(),
            on_changed: RefCell::default(),
            loaded: Cell::new(true),
            inline: RefCell::default(),
        });
        let weak = Rc::downgrade(&this);
        this.button.connect_clicked(move |button| {
            if let Some(this) = weak.upgrade() {
                let parent = button.root().and_downcast::<gtk::Window>();
                gtk::glib::MainContext::default().spawn_local(async move {
                    this.pick(parent.as_ref()).await;
                });
            }
        });
        this.render();
        this
    }

    pub fn load(self: &Rc<Self>, draft: &Draft) {
        self.clear();
        self.loaded.set(draft.attachments_loaded);
        for (index, name) in draft.attachment_names.iter().enumerate() {
            self.items.borrow_mut().push(Item {
                name: name.clone(),
                size: None,
                part: None,
                source: Some(index),
            });
        }
        for part in &draft.attachments {
            if let Ok(mail) = mailparse::parse_mail(part.as_bytes()) {
                self.items.borrow_mut().push(Item {
                    name: crate::models::attachment::name(&mail)
                        .unwrap_or_else(|| "Attachment".into()),
                    size: mail.get_body_raw().ok().map(|bytes| bytes.len() as u64),
                    part: Some(part.clone()),
                    source: None,
                });
            }
        }
        self.render();
    }

    pub fn clear(self: &Rc<Self>) {
        self.revision.set(self.revision.get() + 1);
        self.items.borrow_mut().clear();
        self.removed.borrow_mut().clear();
        self.inline.borrow_mut().clear();
        self.loaded.set(true);
        self.render();
    }

    pub fn connect_changed(&self, action: impl Fn() + 'static) {
        *self.on_changed.borrow_mut() = Some(Box::new(action));
    }

    fn changed(&self) {
        if let Some(action) = self.on_changed.borrow().as_ref() {
            action();
        }
    }

    pub fn loaded(&self) -> bool {
        self.loaded.get()
    }

    pub fn resolve(self: &Rc<Self>, parts: &[String]) {
        if self.loaded.replace(true) {
            return;
        }
        let mut originals = Vec::new();
        for part in parts {
            if let Ok(mail) = mailparse::parse_mail(part.as_bytes()) {
                if crate::models::attachment::name(&mail).is_some() {
                    originals.push(part.clone());
                } else {
                    self.inline.borrow_mut().push(part.clone());
                }
            }
        }
        for item in self.items.borrow_mut().iter_mut() {
            if let Some(index) = item.source.take() {
                item.part = originals.get(index).cloned();
            }
        }
        self.removed.borrow_mut().clear();
        self.render();
    }

    pub fn connect_loading(&self, action: impl Fn(bool) + 'static) {
        *self.on_loading.borrow_mut() = Some(Box::new(action));
    }

    pub fn loading(&self) -> bool {
        self.loading.get()
    }

    pub fn parts(&self) -> Vec<String> {
        self.items
            .borrow()
            .iter()
            .filter_map(|item| item.part.clone())
            .chain(self.inline.borrow().iter().cloned())
            .collect()
    }

    pub fn removed(&self) -> Vec<usize> {
        self.removed.borrow().clone()
    }

    pub fn names(&self) -> Vec<String> {
        self.items
            .borrow()
            .iter()
            .filter(|item| item.source.is_some())
            .map(|item| item.name.clone())
            .collect()
    }

    fn render(self: &Rc<Self>) {
        self.cards.borrow_mut().clear();
        self.grid.remove_all();
        for (index, item) in self.items.borrow().iter().enumerate() {
            let card = super::attachments::FileCard::new(&item.name, item.size);
            let remove = super::button("window-close-symbolic", &format!("Remove {}", item.name));
            let weak = Rc::downgrade(self);
            remove.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    let item = this.items.borrow_mut().remove(index);
                    if let Some(source) = item.source {
                        this.removed.borrow_mut().push(source);
                    }
                    this.render();
                    this.changed();
                }
            });
            card.actions.append(&remove);
            self.grid.append(&card.widget);
            self.cards.borrow_mut().push(card);
        }

        let total: u64 = self
            .items
            .borrow()
            .iter()
            .filter_map(|item| item.size)
            .sum();
        self.widget.set_visible(!self.items.borrow().is_empty());
        self.status.set_text(&if self.items.borrow().is_empty() {
            "Up to 20 MB total".into()
        } else if self.items.borrow().iter().any(|item| item.size.is_none()) {
            file_count(self.items.borrow().len())
        } else {
            format!(
                "{} · {}",
                file_count(self.items.borrow().len()),
                gtk::glib::format_size(total)
            )
        });
    }

    async fn pick(self: Rc<Self>, parent: Option<&gtk::Window>) {
        if self.loading.replace(true) {
            return;
        }
        self.button.set_sensitive(false);
        if let Some(action) = self.on_loading.borrow().as_ref() {
            action(true);
        }
        let revision = self.revision.get();
        let dialog = gtk::FileDialog::builder()
            .title("Attach files")
            .accept_label("Attach")
            .build();
        let result = dialog.open_multiple_future(parent).await;
        if let Ok(files) = &result {
            for file in files.iter::<gtk::gio::File>().filter_map(Result::ok) {
                self.status.set_text("Reading attachment…");
                match self.read(&file).await {
                    Ok(item) if self.revision.get() == revision => {
                        self.items.borrow_mut().push(item);
                        self.render();
                    }
                    Ok(_) => break,
                    Err(error) => {
                        self.status.set_text(&error);
                        break;
                    }
                }
            }
        } else if let Err(error) = result
            && !super::attachments::cancelled(&error)
        {
            self.status
                .set_text(&format!("Could not choose files: {error}"));
        }
        self.loading.set(false);
        self.button.set_sensitive(true);
        if let Some(action) = self.on_loading.borrow().as_ref() {
            action(false);
        }
    }

    async fn read(&self, file: &gtk::gio::File) -> Result<Item, String> {
        let info = file
            .query_info_future(
                "standard::name,standard::size,standard::type,standard::content-type",
                gtk::gio::FileQueryInfoFlags::NONE,
                gtk::glib::Priority::DEFAULT,
            )
            .await
            .map_err(|error| error.to_string())?;
        if info.file_type() != gtk::gio::FileType::Regular {
            return Err("Choose a regular file".into());
        }
        let total: u64 = self
            .items
            .borrow()
            .iter()
            .filter_map(|item| item.size)
            .sum();
        if total + info.size() as u64 > attachments::MAX_BYTES as u64 {
            return Err("Attachments exceed the 20 MB total limit".into());
        }
        let (bytes, _) = file
            .load_contents_future()
            .await
            .map_err(|error| error.to_string())?;
        if total + bytes.len() as u64 > attachments::MAX_BYTES as u64 {
            return Err("Attachments exceed the 20 MB total limit".into());
        }
        let name = info.name().to_string_lossy().into_owned();
        let mime = info
            .content_type()
            .and_then(|content_type| gtk::gio::content_type_get_mime_type(&content_type))
            .unwrap_or_else(|| "application/octet-stream".into());
        let part = attachments::encode(&name, &mime, &bytes).map_err(|error| error.to_string())?;
        Ok(Item {
            name,
            size: Some(bytes.len() as u64),
            part: Some(part),
            source: None,
        })
    }
}

fn file_count(count: usize) -> String {
    format!("{count} {}", if count == 1 { "file" } else { "files" })
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session for attachment rows"]
    fn removes_original_files_by_position_and_keeps_new_binary_parts() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let attachments = Attachments::new();
        let mut draft = Draft::from(&crate::models::Message {
            attachments: vec!["same.txt".into(), "same.txt".into(), "third.txt".into()],
            ..Default::default()
        });
        let part =
            crate::backend::attachments::encode("new.bin", "application/octet-stream", &[0, 255])
                .unwrap();
        draft.attachments.push(part.clone());
        attachments.load(&draft);
        assert_eq!(attachments.cards.borrow().len(), 4);
        let card = attachments.cards.borrow()[0].clone();
        fn remove_button(widget: &gtk::Widget) -> Option<gtk::Button> {
            if let Some(button) = widget.downcast_ref::<gtk::Button>() {
                return Some(button.clone());
            }
            let mut child = widget.first_child();
            while let Some(widget) = child {
                if let Some(button) = remove_button(&widget) {
                    return Some(button);
                }
                child = widget.next_sibling();
            }
            None
        }
        remove_button(card.widget.upcast_ref())
            .unwrap()
            .emit_clicked();
        assert_eq!(attachments.removed(), [0]);
        assert_eq!(attachments.names(), ["same.txt", "third.txt"]);
        assert_eq!(attachments.parts(), [part.clone()]);
        let original_parts = [
            crate::backend::attachments::encode("same.txt", "text/plain", b"first").unwrap(),
            crate::backend::attachments::encode("same.txt", "text/plain", b"second").unwrap(),
            crate::backend::attachments::encode("third.txt", "text/plain", b"third").unwrap(),
            "Content-Type: image/png\r\nContent-ID: <inline>\r\nContent-Transfer-Encoding: base64\r\n\r\nAP8=\r\n".into(),
        ];
        attachments.resolve(&original_parts);
        assert!(attachments.loaded());
        assert!(attachments.removed().is_empty());
        assert!(attachments.names().is_empty());
        assert_eq!(
            attachments.parts(),
            [
                original_parts[1].clone(),
                original_parts[2].clone(),
                part,
                original_parts[3].clone()
            ]
        );
        attachments.resolve(&original_parts);
        assert_eq!(attachments.parts().len(), 4);
        let card = attachments.cards.borrow()[0].clone();
        remove_button(card.widget.upcast_ref())
            .unwrap()
            .emit_clicked();
        assert_eq!(attachments.parts().len(), 3);
        assert!(attachments.removed().is_empty());
        attachments.clear();
        assert!(attachments.cards.borrow().is_empty());
        assert!(attachments.removed().is_empty());
        assert!(!attachments.widget.is_visible());
    }
}
