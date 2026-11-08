use super::*;

pub struct Controls {
    pub widget: gtk::Box,
    pub all: gtk::CheckButton,
    pub unsubscribe: gtk::Button,
    count: gtk::Label,
}

impl Controls {
    pub fn new() -> Self {
        let widget = super::super::horizontal("subscriptions-toolbar", theme::SPACING);
        let all = gtk::CheckButton::with_label("Select all");
        let count = super::super::label("", "dim-label");
        let unsubscribe = gtk::Button::with_label("Unsubscribe 0");
        unsubscribe.add_css_class("destructive-action");
        widget.append(&all);
        widget.append(&count);
        widget.append(&unsubscribe);
        Self {
            widget,
            all,
            unsubscribe,
            count,
        }
    }

    pub fn connect(&self, screen: &Rc<Subscriptions>) {
        let weak = Rc::downgrade(screen);
        self.all.connect_toggled(move |check| {
            if let Some(screen) = weak.upgrade()
                && !screen.updating.get()
                && !screen.blocked.get()
            {
                if check.is_active() {
                    screen.selection.select_all();
                    screen.unselect_unavailable();
                } else {
                    screen.selection.unselect_all();
                }
            }
        });
        let weak = Rc::downgrade(screen);
        self.unsubscribe.connect_clicked(move |_| {
            if let Some(screen) = weak.upgrade()
                && !screen.blocked.get()
            {
                screen.request(screen.selected_entries(), false);
            }
        });
    }

    pub fn update(&self, selected: usize, total: usize, blocked: bool) {
        self.all.set_active(selected > 0 && selected == total);
        self.all.set_inconsistent(selected > 0 && selected < total);
        self.all.set_sensitive(!blocked && total > 0);
        self.count
            .set_text(&format!("{selected} of {total} selected"));
        self.unsubscribe
            .set_label(&format!("Unsubscribe {selected}"));
        self.unsubscribe.set_sensitive(!blocked && selected > 0);
    }
}
