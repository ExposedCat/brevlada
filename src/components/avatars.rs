use adw::prelude::*;
use gtk::{gdk, gdk_pixbuf, glib};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

/// Avatars are prepared at twice the size they are drawn at, so the result is
/// either an exact fit on a HiDPI screen or an exact halving on a normal one.
/// Handing the widget a much larger image instead makes GTK minify it in one
/// step at draw time, which is what turns a crisp logo into mush.
const OVERSAMPLE: i32 = 2;
const RETRY_DELAY: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Cache {
    /// Decoded images at their original size. `None` records that the sender
    /// has no avatar, so the widget keeps its initials and no further lookup is
    /// asked for.
    masters: HashMap<String, Option<gdk_pixbuf::Pixbuf>>,
    /// Masters resampled for one avatar size. The few sizes in the window are
    /// shared by every row, so this stays small.
    scaled: HashMap<(String, i32), gdk::Texture>,
    widgets: HashMap<String, Vec<glib::WeakRef<adw::Avatar>>>,
    pending: HashSet<String>,
    retrying: HashSet<String>,
}

/// Fills sender avatars in as lookups come back. Rows are rebuilt often, so
/// widgets are held weakly and pending lookups are shared between every row
/// showing the same sender.
pub struct Avatars {
    cache: RefCell<Cache>,
    request: Box<dyn Fn(&str)>,
}

impl Avatars {
    pub fn new(request: impl Fn(&str) + 'static) -> Rc<Self> {
        Rc::new(Self {
            cache: RefCell::default(),
            request: Box::new(request),
        })
    }

    /// Shows the avatar for `email` on `avatar`, requesting it if this is the
    /// first time the sender is seen. `email` must be a `senders::key`.
    pub fn attach(&self, avatar: &adw::Avatar, email: &str) {
        if email.is_empty() {
            return;
        }
        let mut cache = self.cache.borrow_mut();
        let widgets = cache.widgets.entry(email.to_owned()).or_default();
        widgets.retain(|widget| widget.upgrade().is_some());
        if !widgets
            .iter()
            .any(|widget| widget.upgrade().as_ref() == Some(avatar))
        {
            widgets.push(avatar.downgrade());
        }
        let request = !cache.masters.contains_key(email)
            && !cache.retrying.contains(email)
            && cache.pending.insert(email.to_owned());
        drop(cache);
        self.draw(avatar, email);
        if request {
            (self.request)(email);
        }
    }

    /// Applies a finished lookup to every row still showing that sender.
    pub fn resolved(&self, email: &str, image: Option<Vec<u8>>) {
        let master = image.and_then(|data| decode(&data));
        let mut cache = self.cache.borrow_mut();
        cache.masters.insert(email.to_owned(), master);
        cache.scaled.retain(|(sender, _), _| sender != email);
        cache.pending.remove(email);
        cache.retrying.remove(email);
        let widgets = cache.widgets.entry(email.to_owned()).or_default();
        widgets.retain(|widget| widget.upgrade().is_some());
        let widgets = widgets.clone();
        drop(cache);
        for widget in widgets {
            if let Some(avatar) = widget.upgrade() {
                self.draw(&avatar, email);
            }
        }
    }

    pub fn failed(self: &Rc<Self>, email: &str) {
        let mut cache = self.cache.borrow_mut();
        cache.pending.remove(email);
        if !cache.retrying.insert(email.to_owned()) {
            return;
        }
        drop(cache);
        let weak = Rc::downgrade(self);
        let email = email.to_owned();
        glib::timeout_add_local_once(RETRY_DELAY, move || {
            if let Some(avatars) = weak.upgrade() {
                avatars.retry(&email);
            }
        });
    }

    fn retry(&self, email: &str) {
        let mut cache = self.cache.borrow_mut();
        if !cache.retrying.remove(email) {
            return;
        }
        let visible = cache.widgets.get(email).is_some_and(|widgets| {
            widgets
                .iter()
                .filter_map(glib::WeakRef::upgrade)
                .any(|avatar| avatar.is_mapped())
        });
        let request = visible && cache.pending.insert(email.to_owned());
        drop(cache);
        if request {
            (self.request)(email);
        }
    }

    /// Resamples the cached image to the pixels this particular avatar is drawn
    /// with. Doing it here rather than in the widget keeps a high-resolution
    /// logo sharp, and the result is shared by every avatar of the same size.
    fn draw(&self, avatar: &adw::Avatar, email: &str) {
        let pixels = avatar.size().max(1) * OVERSAMPLE;
        let key = (email.to_owned(), pixels);
        let mut cache = self.cache.borrow_mut();
        if let Some(texture) = cache.scaled.get(&key) {
            let texture = texture.clone();
            drop(cache);
            avatar.set_custom_image(Some(&texture));
            return;
        }
        let Some(Some(master)) = cache.masters.get(email) else {
            drop(cache);
            avatar.set_custom_image(None::<&gdk::Texture>);
            return;
        };
        let scaled = master
            .scale_simple(pixels, pixels, gdk_pixbuf::InterpType::Hyper)
            .unwrap_or_else(|| master.clone());
        let texture = gdk::Texture::for_pixbuf(&scaled);
        cache.scaled.insert(key, texture.clone());
        drop(cache);
        avatar.set_custom_image(Some(&texture));
    }
}

fn decode(data: &[u8]) -> Option<gdk_pixbuf::Pixbuf> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader.write(data).ok()?;
    loader.close().ok()?;
    loader.pixbuf()
}

/// A one pixel PNG, the smallest payload `gdk::Texture` will decode.
#[cfg(test)]
pub(crate) const PIXEL: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x70, 0x68, 0x38, 0x00,
    0x00, 0x02, 0x84, 0x01, 0x81, 0x26, 0x97, 0xA7, 0x2B, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session; maps isolated avatar widgets"]
    fn refresh_updates_existing_widgets_and_failed_lookups_can_retry() {
        adw::init().unwrap();
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let recorder = asked.clone();
        let avatars = Avatars::new(move |email| recorder.borrow_mut().push(email.to_owned()));
        let first = adw::Avatar::new(40, Some("Ada"), true);
        let second = adw::Avatar::new(32, Some("Ada"), true);
        let content = super::super::column("avatar-diagnostic");
        content.append(&first);
        content.append(&second);
        let window = gtk::Window::builder().child(&content).build();
        window.present();
        let context = glib::MainContext::default();
        context.block_on(glib::timeout_future(Duration::from_millis(100)));

        avatars.attach(&first, "ada@example.com");
        avatars.resolved("ada@example.com", Some(PIXEL.to_vec()));
        avatars.attach(&second, "ada@example.com");
        let original = first.custom_image().unwrap();
        let rebuilt = second.custom_image().unwrap();
        let image = decode(PIXEL).unwrap();
        image.fill(0xFF0000FF);
        avatars.resolved(
            "ada@example.com",
            Some(image.save_to_bufferv("png", &[]).unwrap()),
        );
        assert_ne!(first.custom_image().unwrap(), original);
        assert_ne!(second.custom_image().unwrap(), rebuilt);

        let refreshed = first.custom_image().unwrap();
        avatars.failed("ada@example.com");
        avatars.attach(&first, "ada@example.com");
        assert_eq!(asked.borrow().len(), 1);
        assert_eq!(first.custom_image().unwrap(), refreshed);
        avatars.retry("ada@example.com");
        assert_eq!(asked.borrow().len(), 2);
        avatars.retry("ada@example.com");
        assert_eq!(asked.borrow().len(), 2);
        avatars.resolved("ada@example.com", None);
        assert!(first.custom_image().is_none());
        assert!(second.custom_image().is_none());
        avatars.attach(&first, "ada@example.com");
        assert_eq!(asked.borrow().len(), 2);

        let missing = adw::Avatar::new(40, Some("Grace"), true);
        content.append(&missing);
        context.block_on(glib::timeout_future(Duration::from_millis(100)));
        avatars.attach(&missing, "grace@example.com");
        avatars.failed("grace@example.com");
        avatars.retry("grace@example.com");
        assert_eq!(asked.borrow().last().unwrap(), "grace@example.com");
        assert_eq!(asked.borrow().len(), 4);
        window.destroy();
    }

    #[test]
    #[ignore = "Requires a graphical session; constructs widgets without presenting a window"]
    fn asks_once_per_sender_and_fills_every_row_showing_it() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let asked: Rc<RefCell<Vec<String>>> = Rc::default();
        let recorder = asked.clone();
        let avatars = Avatars::new(move |email| recorder.borrow_mut().push(email.to_owned()));
        let list = adw::Avatar::new(40, Some("Ada"), true);
        let card = adw::Avatar::new(32, Some("Ada"), true);
        avatars.attach(&list, "ada@example.com");
        avatars.attach(&card, "ada@example.com");
        avatars.attach(&adw::Avatar::new(40, Some("Nobody"), true), "");
        assert_eq!(*asked.borrow(), vec!["ada@example.com".to_string()]);
        assert!(list.custom_image().is_none());

        avatars.resolved("ada@example.com", Some(PIXEL.to_vec()));
        // Each avatar is handed an image sized for the pixels it draws with,
        // rather than one oversized image that GTK would have to minify.
        let size = |avatar: &adw::Avatar| {
            avatar
                .custom_image()
                .map(|image| (image.intrinsic_width(), image.intrinsic_height()))
        };
        assert_eq!(size(&list), Some((40 * OVERSAMPLE, 40 * OVERSAMPLE)));
        assert_eq!(size(&card), Some((32 * OVERSAMPLE, 32 * OVERSAMPLE)));

        // A rebuilt row is served from the cache without asking again.
        let rebuilt = adw::Avatar::new(40, Some("Ada"), true);
        avatars.attach(&rebuilt, "ada@example.com");
        assert_eq!(size(&rebuilt), size(&list));
        assert_eq!(asked.borrow().len(), 1);

        // A sender with no avatar anywhere keeps its initials and is not retried.
        let grace = adw::Avatar::new(40, Some("Grace"), true);
        avatars.attach(&grace, "grace@example.com");
        avatars.resolved("grace@example.com", None);
        avatars.attach(&grace, "grace@example.com");
        assert!(grace.custom_image().is_none());
        assert_eq!(asked.borrow().len(), 2);

        // Undecodable bytes are treated the same as no avatar at all.
        let broken = adw::Avatar::new(40, Some("Broken"), true);
        avatars.attach(&broken, "broken@example.com");
        avatars.resolved("broken@example.com", Some(b"<html>".to_vec()));
        assert!(broken.custom_image().is_none());
    }
}
