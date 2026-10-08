use crate::models::sender_action::SenderAction;
use adw::prelude::*;
use std::rc::Rc;

pub fn container(child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let host = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .focusable(true)
        .build();
    host.append(child);
    host
}

#[cfg(test)]
pub fn attach<T: Clone + 'static>(
    list: &gtk::ListBox,
    scroll: &gtk::ScrolledWindow,
    sender_at: impl Fn(i32) -> Option<(T, bool)> + 'static,
    activate: impl Fn(T, SenderAction) + 'static,
    is_trusted: impl Fn(&T) -> bool + 'static,
    toggle_trust: impl Fn(T) + 'static,
) {
    let sender_at = Rc::new(sender_at);
    let activate = Rc::new(activate);
    let lookup = sender_at.clone();
    let shortcut_activate = activate.clone();
    let sender_at_menu = sender_at.clone();
    let activate_menu = activate.clone();
    super::mail_shortcuts::attach(list, move |index, action| {
        if let Some((target, false)) = lookup(index) {
            shortcut_activate(target, action);
            true
        } else {
            false
        }
    });
    attach_inner(
        list,
        scroll,
        move |index| sender_at_menu(index),
        move |target, action| activate_menu(target, action),
        is_trusted,
        toggle_trust,
        |_| {
            SenderAction::ALL
                .iter()
                .map(|(action, _)| action.clone())
                .collect()
        },
        |list, _, y| list.row_at_y(y as i32).map(|row| row.index()),
        |list| list.selected_row().map(|row| row.index()),
        |list, index| list.select_row(index.and_then(|index| list.row_at_index(index)).as_ref()),
    );
}

pub fn attach_view<T: Clone + 'static>(
    list: &gtk::ListView,
    scroll: &gtk::ScrolledWindow,
    sender_at: impl Fn(i32) -> Option<(T, bool)> + 'static,
    activate: impl Fn(T, SenderAction) + 'static,
    is_trusted: impl Fn(&T) -> bool + 'static,
    toggle_trust: impl Fn(T) + 'static,
    menu_actions: impl Fn(&T) -> Vec<SenderAction> + 'static,
) {
    let sender_at = Rc::new(sender_at);
    let activate = Rc::new(activate);
    let lookup = sender_at.clone();
    let shortcut_activate = activate.clone();
    super::mail_shortcuts::attach_view(list, move |index, action| {
        if let Some((target, false)) = lookup(index) {
            shortcut_activate(target, action);
            true
        } else {
            false
        }
    });
    attach_inner(
        list,
        scroll,
        move |index| sender_at(index),
        move |target, action| activate(target, action),
        is_trusted,
        toggle_trust,
        menu_actions,
        |list, x, y| {
            let mut widget = list.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(current) = widget {
                if let Some(key) = current.widget_name().strip_prefix("virtual-message-row-") {
                    return super::virtual_list::position(list, key)
                        .map(|position| position as i32);
                }
                widget = current.parent();
            }
            None
        },
        |list| super::virtual_list::selected(list).map(|index| index as i32),
        |list, index| {
            super::virtual_list::selection(list)
                .set_selected(index.map_or(gtk::INVALID_LIST_POSITION, |index| index as u32))
        },
    );
}

fn attach_inner<W: IsA<gtk::Widget> + Clone + 'static, T: Clone + 'static>(
    list: &W,
    scroll: &gtk::ScrolledWindow,
    sender_at: impl Fn(i32) -> Option<(T, bool)> + 'static,
    activate: impl Fn(T, SenderAction) + 'static,
    is_trusted: impl Fn(&T) -> bool + 'static,
    toggle_trust: impl Fn(T) + 'static,
    menu_actions: impl Fn(&T) -> Vec<SenderAction> + 'static,
    hit: impl Fn(&W, f64, f64) -> Option<i32> + 'static,
    selected: impl Fn(&W) -> Option<i32> + 'static,
    select: impl Fn(&W, Option<i32>) + 'static,
) {
    let activate = Rc::new(activate);
    let sender_at = Rc::new(sender_at);
    let is_trusted = Rc::new(is_trusted);
    let toggle_trust = Rc::new(toggle_trust);
    let selected = Rc::new(selected);
    let select = Rc::new(select);
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = list.downgrade();
    let host = scroll.parent().expect("Sender menu container").downgrade();
    let scroll = scroll.downgrade();
    click.connect_pressed(move |gesture, _, x, y| {
        let Some(list) = weak.upgrade() else { return };
        let Some(index) = hit(&list, x, y) else {
            return;
        };
        let Some((sender, bulk)) = sender_at(index) else {
            return;
        };
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let menu = gtk::gio::Menu::new();
        let actions = gtk::gio::SimpleActionGroup::new();
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.insert_action_group("sender", Some(&actions));
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.set_name(Some("message-menu-shortcuts"));
        for (index, action) in menu_actions(&sender).into_iter().enumerate() {
            let name = format!("action{index}");
            let title = action.title(bulk);
            let entry = gtk::gio::MenuItem::new(None, None);
            entry.set_attribute_value("custom", Some(&name.to_variant()));
            let icon = action.icon();
            let shortcut = match (bulk, &action) {
                (false, SenderAction::Archive) => Some("BackSpace"),
                (false, SenderAction::Delete) => Some("Delete"),
                _ => None,
            };
            let item = gtk::gio::SimpleAction::new(&name, None);
            let activate = activate.clone();
            let sender = sender.clone();
            let weak = popover.downgrade();
            item.connect_activate(move |_, _| {
                if let Some(popover) = weak.upgrade() {
                    popover.popdown();
                }
                activate(sender.clone(), action.clone());
            });
            actions.add_action(&item);
            menu.append_item(&entry);
            let detailed_name = format!("sender.{name}");
            let label = shortcut.map(|key| if key == "BackSpace" { "Backspace" } else { key });
            let button = super::menu_item::action(title, icon, label, &detailed_name);
            assert!(popover.add_child(&button, &name));
            if let Some(shortcut) = shortcut {
                shortcuts.add_shortcut(gtk::Shortcut::new(
                    gtk::ShortcutTrigger::parse_string(shortcut),
                    Some(gtk::NamedAction::new(&detailed_name)),
                ));
            }
        }
        if bulk {
            let section = gtk::gio::Menu::new();
            let name = "trust";
            let entry = gtk::gio::MenuItem::new(None, None);
            entry.set_attribute_value("custom", Some(&name.to_variant()));
            section.append_item(&entry);
            menu.append_section(None, &section);
            let trusted = is_trusted(&sender);
            let action = gtk::gio::SimpleAction::new(name, None);
            let toggle_trust = toggle_trust.clone();
            let weak = popover.downgrade();
            action.connect_activate(move |_, _| {
                if let Some(popover) = weak.upgrade() {
                    popover.popdown();
                }
                toggle_trust(sender.clone());
            });
            actions.add_action(&action);
            let title = if trusted { "Don't trust" } else { "Trust" };
            let icon = if trusted {
                "changes-prevent-symbolic"
            } else {
                "changes-allow-symbolic"
            };
            let button = super::menu_item::action(title, icon, None, "sender.trust");
            assert!(popover.add_child(&button, name));
        }
        if !bulk {
            popover.add_controller(shortcuts);
        }
        popover.set_has_arrow(false);
        popover.add_css_class("mail-actions");
        let Some(host) = host.upgrade() else { return };
        let Some(point) = list.compute_point(&host, &gtk::graphene::Point::new(x as f32, y as f32))
        else {
            return;
        };
        popover.set_parent(&host);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
            point.x() as i32,
            point.y() as i32,
            1,
            1,
        )));
        let Some(scroll) = scroll.upgrade() else {
            return;
        };
        let position = scroll.vadjustment().value();
        let selected_index = selected(&list);
        let select = select.clone();
        let restore_list = list.downgrade();
        let restore_host = host.downgrade();
        let restore_scroll = scroll.downgrade();
        popover.connect_closed(move |popover| {
            if let Some(host) = restore_host.upgrade() {
                host.grab_focus();
            }
            let popover = popover.clone();
            let list = restore_list.clone();
            let scroll = restore_scroll.clone();
            let select = select.clone();
            gtk::glib::idle_add_local_once(move || {
                popover.unparent();
                if let Some(list) = list.upgrade() {
                    select(&list, selected_index);
                }
                if let Some(scroll) = scroll.upgrade() {
                    scroll.vadjustment().set_value(position);
                }
            });
        });
        host.grab_focus();
        popover.popup();
    });
    list.add_controller(click);
}

#[cfg(test)]
mod diagnostics {
    use super::*;
    use std::cell::RefCell;

    fn settle() {
        for _ in 0..20 {
            while gtk::glib::MainContext::default().iteration(false) {}
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn action_button(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if let Some(button) = widget.downcast_ref::<gtk::Button>()
            && let Some(content) = button.child()
            && let Some(label) = content
                .first_child()
                .and_then(|icon| icon.next_sibling())
                .and_then(|widget| widget.downcast::<gtk::Label>().ok())
            && label.text() == name
        {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(button) = action_button(&widget, name) {
                return Some(button);
            }
            child = widget.next_sibling();
        }
        None
    }

    fn menu_shortcuts(popover: &gtk::PopoverMenu) -> Option<gtk::ShortcutController> {
        popover
            .observe_controllers()
            .iter::<gtk::glib::Object>()
            .filter_map(Result::ok)
            .filter_map(|controller| controller.downcast::<gtk::ShortcutController>().ok())
            .find(|controller| controller.name().as_deref() == Some("message-menu-shortcuts"))
    }

    #[test]
    #[ignore = "Requires a graphical session; presents an isolated sender menu without mail workers"]
    fn menu_actions_and_dismissal_preserve_selection_and_scroll() {
        gtk::init().unwrap();
        gtk::gio::resources_register_include!("brevlada.gresource").unwrap();
        gtk::IconTheme::for_display(&gtk::gdk::Display::default().unwrap())
            .add_resource_path("/org/gtk/example/icons");
        adw::init().unwrap();
        let css = gtk::CssProvider::new();
        css.load_from_string(crate::theme::CSS);
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let list = gtk::ListBox::new();
        for index in 0..80 {
            list.append(&gtk::Label::new(Some(&format!("Sender {index}"))));
        }
        let scroll = gtk::ScrolledWindow::builder().child(&list).build();
        let host = container(&scroll);
        let window = gtk::Window::builder()
            .default_width(400)
            .default_height(300)
            .child(&host)
            .build();
        let activated = Rc::new(RefCell::new(Vec::new()));
        let recorder = activated.clone();
        let bulk = Rc::new(std::cell::Cell::new(true));
        let lookup_bulk = bulk.clone();
        attach(
            &list,
            &scroll,
            move |index| Some((index.to_string(), lookup_bulk.get())),
            move |sender, action| {
                recorder.borrow_mut().push((sender, action));
            },
            |_| false,
            |_| {},
        );
        window.present();
        settle();
        list.unselect_all();
        list.row_at_index(0).unwrap().grab_focus();
        list.unselect_all();
        let row = list.row_at_index(40).unwrap();
        let point = row
            .compute_point(&list, &gtk::graphene::Point::new(4.0, 4.0))
            .unwrap();
        scroll.vadjustment().set_value(f64::from(point.y()) - 30.0);
        settle();
        let position = scroll.vadjustment().value();
        assert!(position > 0.0);
        let click = list
            .observe_controllers()
            .iter::<gtk::glib::Object>()
            .filter_map(Result::ok)
            .find_map(|controller| controller.downcast::<gtk::GestureClick>().ok())
            .unwrap();
        for (action, title) in &SenderAction::ALL {
            click.emit_by_name::<()>(
                "pressed",
                &[&1i32, &f64::from(point.x()), &f64::from(point.y())],
            );
            settle();
            let popover = host
                .last_child()
                .unwrap()
                .downcast::<gtk::PopoverMenu>()
                .unwrap();
            let button = action_button(popover.upcast_ref(), title).unwrap();
            let content = button
                .downcast_ref::<gtk::Button>()
                .unwrap()
                .child()
                .unwrap();
            let icon = content
                .first_child()
                .unwrap()
                .downcast::<gtk::Image>()
                .unwrap();
            let title_label = icon
                .next_sibling()
                .unwrap()
                .downcast::<gtk::Label>()
                .unwrap();
            assert_eq!(
                title_label
                    .pango_context()
                    .font_description()
                    .unwrap()
                    .weight(),
                gtk::pango::Weight::Normal
            );
            assert!(icon.is_mapped());
            assert!(icon.width() > 0);
            assert!(icon.icon_name().is_some());
            let shortcut: Option<&str> = None;
            assert!(menu_shortcuts(&popover).is_none());
            if *action == SenderAction::MarkRead {
                assert_eq!(
                    icon.icon_name().as_deref(),
                    Some("brevlada-mail-read-symbolic")
                );
            }
            let theme = gtk::IconTheme::for_display(&icon.display());
            let paintable = theme.lookup_icon(
                icon.icon_name().as_deref().unwrap(),
                &[],
                16,
                1,
                gtk::TextDirection::Ltr,
                gtk::IconLookupFlags::empty(),
            );
            assert!(
                paintable.is_symbolic(),
                "{}: {:?}",
                icon.icon_name().unwrap(),
                paintable.file()
            );
            assert_eq!(
                content
                    .last_child()
                    .filter(|widget| widget.has_css_class("mail-action-shortcut"))
                    .map(|widget| widget.downcast::<gtk::Label>().unwrap().text().to_string())
                    .as_deref(),
                shortcut
            );
            button.emit_by_name::<()>("clicked", &[]);
            assert_eq!(
                activated.borrow().last(),
                Some(&("40".into(), action.clone()))
            );
            settle();
            assert!(popover.parent().is_none());
            assert!(list.selected_row().is_none());
            assert_eq!(scroll.vadjustment().value(), position);
            assert_eq!(
                gtk::prelude::RootExt::focus(&window),
                Some(host.clone().upcast())
            );
        }
        click.emit_by_name::<()>(
            "pressed",
            &[&1i32, &f64::from(point.x()), &f64::from(point.y())],
        );
        settle();
        let popover = host
            .last_child()
            .unwrap()
            .downcast::<gtk::PopoverMenu>()
            .unwrap();
        popover.set_visible(false);
        settle();
        assert_eq!(activated.borrow().len(), SenderAction::ALL.len());
        assert!(list.selected_row().is_none());
        assert_eq!(scroll.vadjustment().value(), position);
        bulk.set(false);
        for (action, title) in [
            (SenderAction::MarkRead, "Mark as read"),
            (SenderAction::Spam, "Mark as spam"),
            (SenderAction::Archive, "Archive"),
            (SenderAction::Delete, "Delete"),
        ] {
            click.emit_by_name::<()>(
                "pressed",
                &[&1i32, &f64::from(point.x()), &f64::from(point.y())],
            );
            settle();
            let popover = host
                .last_child()
                .unwrap()
                .downcast::<gtk::PopoverMenu>()
                .unwrap();
            let button = action_button(popover.upcast_ref(), title).unwrap();
            let content = button
                .downcast_ref::<gtk::Button>()
                .unwrap()
                .child()
                .unwrap();
            let expected = match action {
                SenderAction::Archive => Some("Backspace"),
                SenderAction::Delete => Some("Delete"),
                _ => None,
            };
            assert_eq!(
                content
                    .last_child()
                    .filter(|widget| widget.has_css_class("mail-action-shortcut"))
                    .map(|widget| widget.downcast::<gtk::Label>().unwrap().text().to_string())
                    .as_deref(),
                expected
            );
            button.emit_by_name::<()>("clicked", &[]);
            assert_eq!(activated.borrow().last(), Some(&("40".into(), action)));
            settle();
            assert!(list.selected_row().is_none());
            assert_eq!(scroll.vadjustment().value(), position);
        }
        for (key, action) in [
            (gtk::gdk::Key::BackSpace, SenderAction::Archive),
            (gtk::gdk::Key::Delete, SenderAction::Delete),
        ] {
            click.emit_by_name::<()>(
                "pressed",
                &[&1i32, &f64::from(point.x()), &f64::from(point.y())],
            );
            settle();
            let popover = host
                .last_child()
                .unwrap()
                .downcast::<gtk::PopoverMenu>()
                .unwrap();
            let controller = menu_shortcuts(&popover).unwrap();
            let shortcut = (0..controller.n_items())
                .filter_map(|index| controller.item(index)?.downcast::<gtk::Shortcut>().ok())
                .find(|shortcut| {
                    shortcut
                        .trigger()
                        .and_then(|trigger| trigger.downcast::<gtk::KeyvalTrigger>().ok())
                        .is_some_and(|trigger| trigger.keyval() == key)
                })
                .unwrap();
            assert!(shortcut.action().unwrap().activate(
                gtk::ShortcutActionFlags::empty(),
                &popover,
                None
            ));
            assert_eq!(activated.borrow().last(), Some(&("40".into(), action)));
            settle();
            assert!(!popover.is_visible());
        }
        let keys = list
            .observe_controllers()
            .iter::<gtk::glib::Object>()
            .filter_map(Result::ok)
            .filter_map(|controller| controller.downcast::<gtk::EventControllerKey>().ok())
            .find(|controller| controller.name().as_deref() == Some("mail-row-shortcuts"))
            .unwrap();
        row.grab_focus();
        for (key, action) in [
            (gtk::gdk::Key::Delete, SenderAction::Delete),
            (gtk::gdk::Key::BackSpace, SenderAction::Archive),
        ] {
            assert!(keys.emit_by_name::<bool>(
                "key-pressed",
                &[&key, &0u32, &gtk::gdk::ModifierType::empty()]
            ));
            assert_eq!(activated.borrow().last(), Some(&("40".into(), action)));
        }
        bulk.set(true);
        let count = activated.borrow().len();
        for key in [gtk::gdk::Key::Delete, gtk::gdk::Key::BackSpace] {
            assert!(!keys.emit_by_name::<bool>(
                "key-pressed",
                &[&key, &0u32, &gtk::gdk::ModifierType::empty()]
            ));
        }
        assert_eq!(activated.borrow().len(), count);
        bulk.set(false);
        assert!(!keys.emit_by_name::<bool>(
            "key-pressed",
            &[
                &gtk::gdk::Key::Delete,
                &0u32,
                &gtk::gdk::ModifierType::CONTROL_MASK
            ]
        ));
        let entry = gtk::Entry::new();
        host.append(&entry);
        entry.grab_focus();
        assert!(!keys.emit_by_name::<bool>(
            "key-pressed",
            &[
                &gtk::gdk::Key::BackSpace,
                &0u32,
                &gtk::gdk::ModifierType::empty()
            ]
        ));
        assert_eq!(activated.borrow().len(), count);
        window.close();
    }

    #[test]
    #[ignore = "Requires a graphical session for ListView hit testing"]
    fn virtual_row_context_menu_targets_clicked_item() {
        gtk::init().unwrap();
        adw::init().unwrap();
        let list = super::super::virtual_list::new();
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_bind(|_, object| {
            let item = object.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Label::new(Some(&format!("Sender {}", item.position())));
            row.set_widget_name(&format!("virtual-message-row-{}", item.position()));
            item.set_child(Some(&row));
        });
        list.set_factory(Some(&factory));
        super::super::virtual_list::replace(&list, 20, None);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .build();
        let host = container(&scroll);
        let window = gtk::Window::builder()
            .default_width(400)
            .default_height(300)
            .child(&host)
            .build();
        let activated = Rc::new(RefCell::new(Vec::new()));
        let recorder = activated.clone();
        attach_view(
            &list,
            &scroll,
            |index| Some((index, true)),
            move |index, action| recorder.borrow_mut().push((index, action)),
            |_| false,
            |_| {},
            |_| {
                SenderAction::ALL
                    .iter()
                    .map(|(action, _)| action.clone())
                    .collect()
            },
        );
        window.present();
        settle();
        let row = super::super::virtual_list::visible_row(&list, 3).unwrap();
        let point = row
            .compute_point(&list, &gtk::graphene::Point::new(4.0, 4.0))
            .unwrap();
        let click = list
            .observe_controllers()
            .iter::<gtk::glib::Object>()
            .filter_map(Result::ok)
            .find_map(|controller| controller.downcast::<gtk::GestureClick>().ok())
            .unwrap();
        click.emit_by_name::<()>(
            "pressed",
            &[&1i32, &f64::from(point.x()), &f64::from(point.y())],
        );
        settle();
        let popover = host
            .last_child()
            .unwrap()
            .downcast::<gtk::PopoverMenu>()
            .unwrap();
        action_button(popover.upcast_ref(), "Archive all")
            .unwrap()
            .downcast::<gtk::Button>()
            .unwrap()
            .emit_clicked();
        assert_eq!(*activated.borrow(), vec![(3, SenderAction::Archive)]);
        window.close();
    }
}
