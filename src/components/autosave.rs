use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

type Action = Rc<dyn Fn()>;
type Deferred = Box<dyn FnOnce()>;

pub struct Autosave {
    pub status: gtk::Label,
    revision: Cell<u64>,
    saved: Cell<u64>,
    saving: Cell<u64>,
    busy: Cell<bool>,
    paused: Cell<bool>,
    flush: Cell<bool>,
    timer: RefCell<Option<gtk::glib::SourceId>>,
    action: RefCell<Option<Action>>,
    deferred: RefCell<Option<Deferred>>,
}

impl Autosave {
    pub fn new() -> Rc<Self> {
        let status = super::label("", "dim-label");
        status.set_visible(false);
        Rc::new(Self {
            status,
            revision: Cell::new(0),
            saved: Cell::new(0),
            saving: Cell::new(0),
            busy: Cell::new(false),
            paused: Cell::new(false),
            flush: Cell::new(false),
            timer: RefCell::default(),
            action: RefCell::default(),
            deferred: RefCell::default(),
        })
    }

    pub fn connect(&self, action: impl Fn() + 'static) {
        *self.action.borrow_mut() = Some(Rc::new(action));
    }

    pub fn changed(self: &Rc<Self>) {
        self.revision.set(self.revision.get() + 1);
        if self.paused.get() || self.busy.get() {
            return;
        }
        self.schedule();
    }

    fn schedule(self: &Rc<Self>) {
        self.status.set_visible(true);
        self.status.set_text("Unsaved changes");
        self.cancel_timer();
        let weak = Rc::downgrade(self);
        *self.timer.borrow_mut() = Some(gtk::glib::timeout_add_local_once(
            Duration::from_secs(crate::theme::DRAFT_SAVE_SECONDS),
            move || {
                if let Some(this) = weak.upgrade() {
                    this.timer.borrow_mut().take();
                    this.start();
                }
            },
        ));
    }

    fn start(&self) {
        if self.busy.get() || self.saved.get() == self.revision.get() {
            return;
        }
        let action = self.action.borrow().clone();
        if let Some(action) = action {
            self.busy.set(true);
            self.saving.set(self.revision.get());
            self.status.set_visible(true);
            self.status.set_text("Saving draft…");
            action();
        }
    }

    pub fn capture(&self) {
        self.saving.set(self.revision.get());
    }

    pub fn complete(self: &Rc<Self>, result: Result<(), String>) {
        self.busy.set(false);
        let failed = result.is_err();
        if let Err(error) = result {
            self.status
                .set_text(&format!("Could not save draft: {error}"));
            if self.flush.get() {
                self.deferred.borrow_mut().take();
                self.paused.set(false);
                return;
            }
        } else {
            self.saved.set(self.saving.get());
            self.status.set_text("Draft saved");
            if self.flush.get() && self.saved.get() != self.revision.get() {
                self.start();
                return;
            }
        }
        let deferred = self.deferred.borrow_mut().take();
        if let Some(action) = deferred {
            action();
        } else if !self.paused.get()
            && self.saved.get() != self.revision.get()
            && (!failed || self.revision.get() != self.saving.get())
        {
            self.schedule();
        }
    }

    pub fn defer(self: &Rc<Self>, flush: bool, action: impl FnOnce() + 'static) {
        if self.paused.replace(true) {
            return;
        }
        self.cancel_timer();
        self.flush.set(flush);
        *self.deferred.borrow_mut() = Some(Box::new(action));
        if self.busy.get() {
            return;
        }
        if flush && self.saved.get() != self.revision.get() && self.action.borrow().is_some() {
            self.start();
        } else {
            let action = self.deferred.borrow_mut().take();
            if let Some(action) = action {
                action();
            }
        }
    }

    pub fn paused(&self) -> bool {
        self.paused.get()
    }

    pub fn resume(self: &Rc<Self>) {
        self.paused.set(false);
        self.flush.set(false);
        if self.saved.get() != self.revision.get() {
            self.schedule();
        }
    }

    pub fn reset(&self) {
        self.cancel_timer();
        self.saved.set(self.revision.get());
        self.status.set_text("");
        self.status.set_visible(false);
    }

    fn cancel_timer(&self) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.remove();
        }
    }
}

impl Drop for Autosave {
    fn drop(&mut self) {
        self.cancel_timer();
    }
}
