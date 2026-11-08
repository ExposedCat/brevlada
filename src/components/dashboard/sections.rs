use super::controls::Controls;
use super::*;

pub(super) struct SectionView {
    pub widget: gtk::Box,
    cards: Option<gtk::Box>,
    flow: Option<super::super::calendar::Flow>,
    controls: Rc<Controls>,
    items: Vec<(Entry, bool)>,
    pub unsubscribe_buttons: HashMap<String, gtk::Button>,
    widgets: HashMap<String, gtk::Box>,
    show_accounts: bool,
    expanded: bool,
}

fn same_card(left: &Entry, right: &Entry) -> bool {
    left.id == right.id
        && left.source == right.source
        && left.content == right.content
        && left.series == right.series
}

impl SectionView {
    fn new(dashboard: &Dashboard, section: Section) -> Self {
        let widget = column("dashboard-section");
        widget.set_spacing(theme::SMALL_SPACING);
        let heading = horizontal("dashboard-section-heading", theme::SMALL_SPACING);
        heading.append(&label(section.title(), "title-3"));
        widget.append(&heading);
        let cards = (section == Section::Subscriptions).then(|| {
            let cards = column("dashboard-subscription-cards");
            cards.set_spacing(theme::SPACING);
            widget.append(&cards);
            cards
        });
        let controls = Controls::new(dashboard, section);
        widget.append(&controls.widget);
        Self {
            widget,
            cards,
            flow: None,
            controls,
            items: Vec::new(),
            widgets: HashMap::new(),
            unsubscribe_buttons: HashMap::new(),
            show_accounts: false,
            expanded: false,
        }
    }

    pub fn clear(&mut self) {
        if let Some(cards) = &self.cards {
            super::super::clear(cards);
        }
        if let Some(flow) = self.flow.take() {
            self.widget.remove(&flow);
        }
        self.widgets.clear();
        self.unsubscribe_buttons.clear();
        self.items.clear();
    }

    pub fn pending(&mut self, id: &str, pending: bool) {
        if let Some((_, value)) = self.items.iter_mut().find(|(entry, _)| entry.id == id) {
            *value = pending;
        }
    }

    fn update(
        &mut self,
        items: Vec<(Entry, bool)>,
        show_accounts: bool,
        expanded: bool,
    ) -> Vec<Tile> {
        if self.items.len() == items.len()
            && self
                .items
                .iter()
                .zip(&items)
                .all(|((old, old_pending), (entry, pending))| {
                    same_card(old, entry) && old_pending == pending
                })
            && self.show_accounts == show_accounts
            && self.expanded == expanded
        {
            return Vec::new();
        }
        let mut tiles = Vec::new();
        if let Some(cards) = &self.cards {
            let removed: Vec<_> = self
                .widgets
                .keys()
                .filter(|id| !items.iter().any(|(entry, _)| &entry.id == *id))
                .cloned()
                .collect();
            for id in removed {
                cards.remove(&self.widgets.remove(&id).unwrap());
                self.unsubscribe_buttons.remove(&id);
            }
            let mut previous: Option<gtk::Widget> = None;
            for (entry, pending) in &items {
                let unchanged = self.show_accounts == show_accounts
                    && self
                        .items
                        .iter()
                        .any(|(old, old_pending)| same_card(old, entry) && old_pending == pending);
                if !unchanged {
                    if let Some(old) = self.widgets.remove(&entry.id) {
                        cards.remove(&old);
                    }
                    let tile = Tile::new(entry, show_accounts, *pending);
                    cards.insert_child_after(&tile.widget, previous.as_ref());
                    self.widgets.insert(entry.id.clone(), tile.widget.clone());
                    if let Some(button) = &tile.unsubscribe {
                        self.unsubscribe_buttons
                            .insert(entry.id.clone(), button.clone());
                    }
                    tiles.push(tile);
                }
                let widget = self.widgets.get(&entry.id).unwrap();
                if widget.prev_sibling() != previous {
                    cards.reorder_child_after(widget, previous.as_ref());
                }
                previous = Some(widget.clone().upcast());
            }
        } else {
            if let Some(flow) = self.flow.take() {
                self.widget.remove(&flow);
            }
            tiles = items
                .iter()
                .map(|(entry, pending)| Tile::new(entry, show_accounts, *pending))
                .collect();
            let widgets: Vec<_> = tiles
                .iter()
                .map(|tile| tile.widget.clone().upcast())
                .collect();
            let flow = super::super::calendar::Flow::grid(
                &widgets,
                if expanded { 0 } else { theme::DASHBOARD_ROWS },
            );
            let controls = Rc::downgrade(&self.controls);
            flow.connect_visible_items_notify(move |flow| {
                if let Some(controls) = controls.upgrade() {
                    controls.show(flow.visible_items() as usize);
                }
            });
            self.widget
                .insert_child_after(&flow, self.widget.first_child().as_ref());
            self.flow = Some(flow);
        }
        self.items = items;
        self.show_accounts = show_accounts;
        self.expanded = expanded;
        tiles
    }
}

impl Dashboard {
    pub(super) fn section(
        &self,
        section: Section,
        items: Vec<&Entry>,
        show_accounts: bool,
        pending: &impl Fn(&Entry) -> bool,
    ) -> (gtk::Box, Vec<Tile>) {
        let count = items.len();
        let expanded_limit = self.expanded_limits.borrow().get(&section).copied();
        let subscriptions = section == Section::Subscriptions;
        let limit =
            expanded_limit.unwrap_or(theme::DASHBOARD_ROWS * if subscriptions { 1 } else { 2 });
        let items = items
            .into_iter()
            .take(limit)
            .map(|entry| (entry.clone(), pending(entry)))
            .collect();
        let mut sections = self.sections.borrow_mut();
        let view = sections
            .entry(section)
            .or_insert_with(|| SectionView::new(self, section));
        let tiles = view.update(items, show_accounts, expanded_limit.is_some());
        let shown = view
            .flow
            .as_ref()
            .map_or(view.items.len(), |flow| flow.visible_items() as usize);
        view.controls.update(count, shown, expanded_limit.is_some());
        (view.widget.clone(), tiles)
    }
}
