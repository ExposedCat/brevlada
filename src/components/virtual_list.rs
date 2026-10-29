use crate::models::Message;
use adw::prelude::*;
use gtk::{gio, glib};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Default)]
pub struct Row {
    pub key: String,
    pub messages: Vec<Message>,
}

pub fn with_item<T>(
    list: &gtk::ListView,
    position: u32,
    inspect: impl FnOnce(&Row) -> T,
) -> Option<T> {
    let object = model(list)
        .item(position)?
        .downcast::<glib::BoxedAnyObject>()
        .ok()?;
    Some(inspect(&object.borrow::<Row>()))
}

pub fn item(list: &gtk::ListView, position: u32) -> Option<Row> {
    with_item(list, position, Clone::clone)
}

pub fn position(list: &gtk::ListView, key: &str) -> Option<u32> {
    (0..model(list).n_items())
        .find(|&position| with_item(list, position, |row| row.key == key).unwrap_or(false))
}

fn event_position(list: &gtk::ListView, event: &gtk::gdk::Event) -> Option<u32> {
    let (x, y) = event.position()?;
    let native = list.native()?;
    let (dx, dy) = native.surface_transform();
    let widget = native.dynamic_cast::<gtk::Widget>().ok()?;
    let point = widget.compute_point(
        list,
        &gtk::graphene::Point::new((x + dx) as f32, (y + dy) as f32),
    )?;
    position_at(list, f64::from(point.x()), f64::from(point.y()))
}

pub fn position_at(list: &gtk::ListView, x: f64, y: f64) -> Option<u32> {
    let mut widget = list.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(current) = widget {
        if let Some(key) = current.widget_name().strip_prefix("virtual-message-row-") {
            return position(list, key);
        }
        widget = current.parent();
    }
    None
}

pub fn identify(row: &impl IsA<gtk::Widget>, key: &str) {
    row.set_widget_name(&format!("virtual-message-row-{key}"));
}

pub fn groups(list: &gtk::ListView) -> Vec<Vec<Message>> {
    (0..model(list).n_items())
        .filter_map(|position| item(list, position).map(|row| row.messages))
        .collect()
}

pub fn new() -> gtk::ListView {
    let model = gio::ListStore::new::<glib::BoxedAnyObject>();
    let selection = gtk::MultiSelection::new(Some(model));
    let list = gtk::ListView::builder()
        .model(&selection)
        .css_classes(["boxed-list", "message-list-view"])
        .build();
    let click = gtk::EventControllerLegacy::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = list.downgrade();
    click.connect_event(move |_, event| {
        if event.event_type() == gtk::gdk::EventType::ButtonRelease
            && let Some(button) = event.downcast_ref::<gtk::gdk::ButtonEvent>()
            && button.button() == gtk::gdk::BUTTON_PRIMARY
            && !event.modifier_state().intersects(
                gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK,
            )
            && let Some(list) = weak.upgrade()
            && let Some(position) = event_position(&list, event)
        {
            let key = with_item(&list, position, |row| row.key.clone());
            let weak = list.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(list) = weak.upgrade()
                    && with_item(&list, position, |row| row.key.clone()) == key
                {
                    list.emit_by_name::<()>("activate", &[&position]);
                }
            });
        }
        glib::Propagation::Proceed
    });
    list.add_controller(click);
    list
}

pub fn scroll(list: &gtk::ListView) -> gtk::ScrolledWindow {
    let scroll = super::scroll(list);
    scroll.set_propagate_natural_height(true);
    scroll.set_valign(gtk::Align::Start);
    scroll.set_overflow(gtk::Overflow::Hidden);
    scroll.add_css_class("message-list-surface");
    scroll
}

pub fn selection(list: &gtk::ListView) -> gtk::MultiSelection {
    list.model()
        .expect("List view has a model")
        .downcast()
        .expect("List view has multiple selection")
}

pub fn model(list: &gtk::ListView) -> gio::ListStore {
    selection(list)
        .model()
        .expect("Selection has a model")
        .downcast()
        .expect("Selection has a row store")
}

pub fn selected_positions(list: &gtk::ListView) -> Vec<u32> {
    let selected = selection(list).selection();
    (0..selected.size())
        .map(|index| selected.nth(index as u32))
        .collect()
}

pub fn selected_keys(list: &gtk::ListView) -> Vec<String> {
    selected_positions(list)
        .into_iter()
        .filter_map(|position| with_item(list, position, |row| row.key.clone()))
        .collect()
}

pub fn restore_selection(list: &gtk::ListView, keys: &[String]) {
    let selected = gtk::Bitset::new_empty();
    let keys: HashSet<_> = keys.iter().map(String::as_str).collect();
    for position in 0..model(list).n_items() {
        if with_item(list, position, |row| keys.contains(row.key.as_str())).unwrap_or(false) {
            selected.add(position);
        }
    }
    selection(list).set_selection(&selected, &gtk::Bitset::new_range(0, model(list).n_items()));
}

pub fn select(list: &gtk::ListView, position: u32) {
    if position == gtk::INVALID_LIST_POSITION {
        selection(list).unselect_all();
    } else {
        selection(list).select_item(position, true);
    }
}

pub fn selected(list: &gtk::ListView) -> Option<u32> {
    let selected = selection(list).selection();
    (!selected.is_empty()).then(|| selected.minimum())
}

pub fn focus_selected(list: &gtk::ListView) {
    if let Some(position) = selected(list) {
        list.grab_focus();
        list.scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
    }
}

pub fn reveal(list: &gtk::ListView, position: u32) {
    let key = with_item(list, position, |row| row.key.clone());
    select(list, position);
    list.scroll_to(position, gtk::ListScrollFlags::SELECT, None);
    let laid_out = std::cell::Cell::new(false);
    list.add_tick_callback(move |list, _| {
        if !laid_out.replace(true) {
            return glib::ControlFlow::Continue;
        }
        if selected(list) == Some(position)
            && with_item(list, position, |row| row.key.clone()) == key
        {
            list.scroll_to(position, gtk::ListScrollFlags::SELECT, None);
        }
        glib::ControlFlow::Break
    });
}

pub fn replace(list: &gtk::ListView, count: usize, selected: Option<usize>) {
    let rows: Vec<_> = (0..count)
        .map(|index| Row {
            key: index.to_string(),
            messages: Vec::new(),
        })
        .collect();
    update_owned(list, rows, &[], selected);
}

#[cfg(test)]
pub fn update(list: &gtk::ListView, rows: &[Row], changed: &[usize], selected: Option<usize>) {
    update_owned(list, rows.to_vec(), changed, selected);
}

/// Move worker results into the list model without copying message bodies on GTK.
pub fn update_owned(
    list: &gtk::ListView,
    rows: Vec<Row>,
    changed: &[usize],
    selected: Option<usize>,
) {
    let selected_keys = selected_keys(list);
    let restore_focus = list
        .root()
        .and_then(|root| root.focus())
        .is_some_and(|focus| {
            (focus == *list.upcast_ref::<gtk::Widget>() || focus.is_ancestor(list))
                && !focus.is::<gtk::Popover>()
                && focus.ancestor(gtk::Popover::static_type()).is_none()
        });
    // Keep focus off rows while splicing: removing the focused row otherwise
    // makes GTK focus (and scroll to) the first row in the list.
    if restore_focus && let Some(root) = list.root() {
        root.set_focus(Some(list));
    }
    let model = model(list);
    let objects: Vec<_> = (0..model.n_items())
        .map(|position| {
            model
                .item(position)
                .unwrap()
                .downcast::<glib::BoxedAnyObject>()
                .unwrap()
        })
        .collect();
    let old: Vec<_> = objects
        .iter()
        .map(|object| object.borrow::<Row>().key.clone())
        .collect();
    let keys: Vec<_> = rows.iter().map(|row| row.key.clone()).collect();
    assert_eq!(
        keys.iter().collect::<HashSet<_>>().len(),
        keys.len(),
        "List rows must have unique identities"
    );
    let by_key: HashMap<_, _> = old
        .iter()
        .zip(&objects)
        .map(|(key, object)| (key.as_str(), object))
        .collect();
    let changed_set: HashSet<_> = changed.iter().copied().collect();
    let additions: Vec<_> = rows
        .into_iter()
        .enumerate()
        .map(|(index, row)| {
            if !changed_set.contains(&index)
                && let Some(object) = by_key.get(row.key.as_str())
            {
                object.replace(row);
                (*object).clone()
            } else {
                glib::BoxedAnyObject::new(row)
            }
        })
        .collect();
    let positions: HashMap<&str, usize> = keys
        .iter()
        .enumerate()
        .map(|(i, key)| (key.as_str(), i))
        .collect();
    let matches: Vec<(usize, usize)> = old
        .iter()
        .enumerate()
        .filter_map(|(i, key)| positions.get(key.as_str()).map(|&j| (i, j)))
        .collect();
    let mut tails: Vec<usize> = Vec::new();
    let mut previous = vec![None; matches.len()];
    for (index, &(_, new_index)) in matches.iter().enumerate() {
        let place = tails.partition_point(|&tail| matches[tail].1 < new_index);
        if place > 0 {
            previous[index] = Some(tails[place - 1]);
        }
        if place == tails.len() {
            tails.push(index);
        } else {
            tails[place] = index;
        }
    }
    let mut kept = Vec::new();
    let mut cursor = tails.last().copied();
    while let Some(index) = cursor {
        kept.push(matches[index]);
        cursor = previous[index];
    }
    kept.reverse();
    let retained: HashSet<usize> = kept.iter().map(|&(_, index)| index).collect();
    let mut anchors = Vec::with_capacity(kept.len() + 2);
    anchors.push((usize::MAX, usize::MAX));
    anchors.extend(kept);
    anchors.push((old.len(), keys.len()));
    for pair in anchors.windows(2).rev() {
        let (before_old, _) = pair[0];
        let (after_old, _) = pair[1];
        let start_old = if before_old == usize::MAX {
            0
        } else {
            before_old + 1
        };
        if start_old != after_old {
            model.splice(
                start_old as u32,
                (after_old - start_old) as u32,
                &[] as &[glib::BoxedAnyObject],
            );
        }
    }
    for (position, pair) in anchors.windows(2).enumerate().rev() {
        let (_, before_new) = pair[0];
        let (_, after_new) = pair[1];
        let start_new = if before_new == usize::MAX {
            0
        } else {
            before_new + 1
        };
        if start_new != after_new {
            model.splice(position as u32, 0, &additions[start_new..after_new]);
        }
    }
    for &index in changed {
        if retained.contains(&index) {
            model.splice(index as u32, 1, &additions[index..index + 1]);
        }
    }
    if selected_keys.len() > 1 {
        restore_selection(list, &selected_keys);
    } else {
        select(
            list,
            selected
                .filter(|position| *position < keys.len())
                .map_or(gtk::INVALID_LIST_POSITION, |position| position as u32),
        );
    }
    if restore_focus {
        focus_selected(list);
    }
}

#[cfg(test)]
pub fn visible_row(list: &gtk::ListView, position: u32) -> Option<gtk::Widget> {
    fn find(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if widget.widget_name() == name {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(found) = find(&widget, name) {
                return Some(found);
            }
            child = widget.next_sibling();
        }
        None
    }
    find(
        list.upcast_ref(),
        &format!("virtual-message-row-{}", item(list, position)?.key),
    )
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[test]
    #[ignore = "Requires a graphical session for GTK selection models"]
    fn multiple_selection_survives_reordering_updates_and_removal() {
        gtk::init().unwrap();
        let list = new();
        let rows: Vec<_> = ["a", "b", "c", "d"]
            .into_iter()
            .map(|key| Row {
                key: key.into(),
                ..Default::default()
            })
            .collect();
        update(&list, &rows, &[], None);
        selection(&list).select_item(0, true);
        selection(&list).select_item(2, false);
        let reordered = vec![
            rows[2].clone(),
            rows[1].clone(),
            rows[3].clone(),
            rows[0].clone(),
        ];
        update(&list, &reordered, &[0, 3], Some(3));
        assert_eq!(selected_keys(&list), vec!["c", "a"]);
        update(&list, &reordered[..3], &[], None);
        assert_eq!(selected_keys(&list), vec!["c"]);
        selection(&list).select_range(0, 3, true);
        assert_eq!(selected_keys(&list), vec!["c", "b", "d"]);
        selection(&list).unselect_item(1);
        assert_eq!(selected_keys(&list), vec!["c", "d"]);
    }

    #[test]
    #[ignore = "Requires a graphical session to inspect list item focus"]
    fn replacing_the_selected_row_keeps_keyboard_focus_on_it() {
        gtk::init().unwrap();
        let list = new();
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_bind(|_, object| {
            let item = object.downcast_ref::<gtk::ListItem>().unwrap();
            let data = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
            let row = data.borrow::<Row>();
            let label = gtk::Label::new(Some(&row.key));
            identify(&label, &row.key);
            item.set_child(Some(&label));
        });
        list.set_factory(Some(&factory));
        let rows: Vec<_> = (0..10)
            .map(|index| Row {
                key: index.to_string(),
                ..Default::default()
            })
            .collect();
        update(&list, &rows, &[], Some(6));
        let reader = gtk::Button::with_label("Reader control");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&scroll(&list));
        content.append(&reader);
        let window = gtk::Window::builder()
            .default_width(300)
            .default_height(400)
            .child(&content)
            .build();
        fn layout() {
            let context = glib::MainContext::default();
            let until = std::time::Instant::now() + std::time::Duration::from_millis(100);
            while std::time::Instant::now() < until {
                while context.pending() {
                    context.iteration(false);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        fn assert_focused(list: &gtk::ListView, window: &gtk::Window) {
            let row = visible_row(list, 6).unwrap().parent().unwrap();
            let focus = gtk::prelude::RootExt::focus(window).unwrap();
            assert!(
                focus == row || focus.is_ancestor(&row),
                "focused {} instead of selected row",
                focus.type_().name()
            );
        }
        window.present();
        layout();
        list.scroll_to(6, gtk::ListScrollFlags::FOCUS, None);
        layout();
        assert_focused(&list, &window);
        update(&list, &rows, &[6], Some(6));
        layout();
        assert_eq!(selected(&list), Some(6));
        assert_focused(&list, &window);
        let remaining: Vec<_> = rows.into_iter().filter(|row| row.key != "6").collect();
        update(&list, &remaining, &[], Some(6));
        layout();
        assert_focused(&list, &window);
        reader.grab_focus();
        update(&list, &remaining, &[6], Some(6));
        layout();
        assert_eq!(
            gtk::prelude::RootExt::focus(&window),
            Some(reader.clone().upcast())
        );
        focus_selected(&list);
        layout();
        assert_focused(&list, &window);
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session for GTK models"]
    fn sorting_filtering_and_paging_never_publish_duplicate_or_mismatched_rows() {
        gtk::init().unwrap();
        let list = new();
        let weak = list.downgrade();
        model(&list).connect_items_changed(move |_, _, _, _| {
            let list = weak.upgrade().unwrap();
            let mut keys = HashSet::new();
            for position in 0..model(&list).n_items() {
                let row = item(&list, position).unwrap();
                assert!(
                    keys.insert(row.key.clone()),
                    "duplicate row during a model update"
                );
                assert_eq!(row.key, crate::models::senders::key(&row.messages[0]));
            }
        });
        let rows: Vec<_> = (0..100)
            .map(|uid| {
                let sender = format!("sender{uid}@example.com");
                Row {
                    key: sender.clone(),
                    messages: vec![Message {
                        uid,
                        sender,
                        ..Default::default()
                    }],
                }
            })
            .collect();
        update(&list, &rows, &[], None);
        for offset in 0..100 {
            let mut next = rows.clone();
            next.rotate_left(offset);
            if offset % 2 == 0 {
                next.reverse();
            }
            next.retain(|row| row.messages[0].uid as usize % 3 != offset % 3);
            update(&list, &next, &[], None);
            assert_eq!(
                (0..model(&list).n_items())
                    .map(|position| item(&list, position).unwrap().key)
                    .collect::<Vec<_>>(),
                next.iter().map(|row| row.key.clone()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    #[ignore = "Requires a graphical session for GTK models"]
    fn inserting_between_rows_keeps_existing_model_items() {
        gtk::init().unwrap();
        let list = new();
        let before: Vec<Row> = ["a", "b", "c", "d"]
            .into_iter()
            .map(|key| Row {
                key: key.into(),
                ..Default::default()
            })
            .collect();
        update(&list, &before, &[], None);
        let changes = Rc::new(RefCell::new(Vec::new()));
        let recorded = changes.clone();
        model(&list).connect_items_changed(move |_, position, removed, added| {
            recorded.borrow_mut().push((position, removed, added))
        });
        let after: Vec<Row> = ["a", "b", "new", "c", "d"]
            .into_iter()
            .map(|key| Row {
                key: key.into(),
                ..Default::default()
            })
            .collect();
        update(&list, &after, &[], None);
        assert_eq!(*changes.borrow(), vec![(2, 0, 1)]);
    }

    #[test]
    #[ignore = "Requires a graphical session to measure realized list rows"]
    fn only_visible_rows_are_bound() {
        gtk::init().unwrap();
        let list = new();
        let binds = Rc::new(Cell::new(0));
        let factory = gtk::SignalListItemFactory::new();
        let count = binds.clone();
        factory.connect_bind(move |_, object| {
            count.set(count.get() + 1);
            let item = object.downcast_ref::<gtk::ListItem>().unwrap();
            item.set_child(Some(&gtk::Label::new(Some(&item.position().to_string()))));
        });
        list.set_factory(Some(&factory));
        replace(&list, 1000, None);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .min_content_height(300)
            .build();
        let window = gtk::Window::builder()
            .default_width(300)
            .default_height(300)
            .child(&scroll)
            .build();
        window.present();
        let context = gtk::glib::MainContext::default();
        for _ in 0..20 {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(binds.get() > 0);
        assert!(
            binds.get() < 300,
            "bound {} rows for 1000 items",
            binds.get()
        );
        let adjustment = scroll.vadjustment();
        adjustment.set_value(3000.0);
        for _ in 0..10 {
            while context.pending() {
                context.iteration(false);
            }
        }
        let before = adjustment.value();
        let keys: Vec<_> = (0..1050)
            .map(|index| Row {
                key: index.to_string(),
                ..Default::default()
            })
            .collect();
        update(&list, &keys, &[150], None);
        for _ in 0..10 {
            while context.pending() {
                context.iteration(false);
            }
        }
        assert!((adjustment.value() - before).abs() < 1.0);
        window.close();
    }
}
