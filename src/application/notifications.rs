use crate::{
    components::display,
    models::{Message, sender_action::SenderAction, senders},
};
use adw::prelude::*;
use gtk::{gio, glib};
use std::collections::{HashMap, HashSet};

const ACTIONS: [(&str, SenderAction); 3] = [
    ("notification-mark-read", SenderAction::MarkRead),
    ("notification-archive", SenderAction::Archive),
    ("notification-delete", SenderAction::Delete),
];

pub(super) fn install_actions(app: &adw::Application, engine: &std::rc::Rc<super::Engine>) {
    for (name, action) in ACTIONS {
        let handler =
            gio::SimpleAction::new(name, Some(&<(String, u32, u32)>::static_variant_type()));
        let weak = std::rc::Rc::downgrade(engine);
        handler.connect_activate(move |_, target| {
            if let Some(engine) = weak.upgrade()
                && let Some((account, validity, uid)) =
                    target.and_then(|target| target.get::<(String, u32, u32)>())
            {
                let result = engine.worker.send(super::Command::NotificationAction {
                    account: account.clone(),
                    validity,
                    uid,
                    action: action.clone(),
                });
                engine.event(match result {
                    Ok(()) => super::Event::NotificationActionStarted {
                        account,
                        validity,
                        uid,
                        action: action.clone(),
                    },
                    Err(error) => super::Event::NotificationActionFinished(
                        account,
                        validity,
                        uid,
                        Some(error.to_string()),
                    ),
                });
            }
        });
        app.add_action(&handler);
    }
}

pub(super) struct Delivery {
    pub id: String,
    pub title: String,
    pub body: String,
    pub uid: u32,
    pub sender: Option<String>,
}

impl Delivery {
    pub fn notification(
        &self,
        account: &str,
        validity: u32,
        icons: &HashMap<String, Vec<u8>>,
    ) -> gio::Notification {
        let notification = gio::Notification::new(&self.title);
        notification.set_body(Some(&self.body));
        if let Some(image) = self.sender.as_ref().and_then(|sender| icons.get(sender)) {
            notification.set_icon(&gio::BytesIcon::new(&glib::Bytes::from(image.as_slice())));
        } else {
            notification.set_icon(&gio::ThemedIcon::new(
                "io.github.ExposedCat.Brevlada-symbolic",
            ));
        }
        let target = (account, validity, self.uid).to_variant();
        notification.set_default_action_and_target_value("app.open-message", Some(&target));
        if self.sender.is_some() {
            for (name, action) in ACTIONS {
                notification.add_button_with_target_value(
                    action.title(false),
                    &format!("app.{name}"),
                    Some(&target),
                );
            }
        }
        notification
    }
}

pub(super) fn deliveries(account: &str, validity: u32, messages: &[&Message]) -> Vec<Delivery> {
    if messages.len() <= 5 {
        messages
            .iter()
            .map(|message| Delivery {
                id: format!("new-mail-{account}-{validity}-{}", message.uid),
                title: message.sender.clone(),
                body: message.subject.clone(),
                uid: message.uid,
                sender: Some(senders::key(message)),
            })
            .collect()
    } else {
        vec![Delivery {
            id: format!("new-mail-{account}"),
            title: format!("{} new messages", messages.len()),
            body: grouped_body(account, messages),
            uid: messages[0].uid,
            sender: None,
        }]
    }
}

pub(super) fn grouped_body(account: &str, messages: &[&Message]) -> String {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for message in messages {
        if seen.insert(senders::key(message)) && names.len() < 5 {
            names.push(display::sender_name(message));
        }
    }
    let mut body = format!("{account} received messages from {}", names.join(", "));
    if seen.len() > 5 {
        body.push_str(&format!(" ({} others)", seen.len() - 5));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(sender: &str) -> Message {
        Message {
            sender: sender.into(),
            ..Message::default()
        }
    }

    #[test]
    fn sends_up_to_five_distinct_message_notifications_then_one_summary() {
        let messages: Vec<_> = (1..=6)
            .map(|uid| Message {
                uid,
                sender: format!("Sender {uid}"),
                subject: format!("Subject {uid}"),
                ..Message::default()
            })
            .collect();
        for count in 0..=5 {
            let notifications =
                deliveries("account", 7, &messages[..count].iter().collect::<Vec<_>>());
            assert_eq!(notifications.len(), count);
            let ids: HashSet<_> = notifications.iter().map(|item| &item.id).collect();
            assert_eq!(ids.len(), count);
            for (notification, message) in notifications.iter().zip(&messages) {
                assert_eq!(notification.title, message.sender);
                assert_eq!(notification.body, message.subject);
                assert_eq!(notification.uid, message.uid);
                assert_eq!(
                    notification.sender.as_deref(),
                    Some(senders::key(message).as_str())
                );
                assert_ne!(notification.id, deliveries("other", 7, &[message])[0].id);
                assert_ne!(notification.id, deliveries("account", 8, &[message])[0].id);
            }
        }
        let notifications = deliveries("account", 7, &messages.iter().collect::<Vec<_>>());
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].title, "6 new messages");
        assert_eq!(notifications[0].uid, 1);
        assert!(notifications[0].sender.is_none());
        assert!(notifications[0].body.ends_with("(1 others)"));
    }

    #[test]
    fn lists_sender_names_once_per_address_in_message_order() {
        let messages = [
            message("Alice <alice@example.com>"),
            message("Bob <bob@example.com>"),
            message("Older Alice <ALICE@example.com>"),
        ];
        assert_eq!(
            grouped_body("me@example.com", &messages.iter().collect::<Vec<_>>()),
            "me@example.com received messages from Alice, Bob"
        );
    }

    #[test]
    fn caps_names_at_five_and_counts_remaining_senders() {
        let messages: Vec<_> = (1..=7)
            .map(|index| message(&format!("Sender {index} <sender{index}@example.com>")))
            .collect();
        assert_eq!(
            grouped_body("me@example.com", &messages[..5].iter().collect::<Vec<_>>()),
            "me@example.com received messages from Sender 1, Sender 2, Sender 3, Sender 4, Sender 5"
        );
        assert_eq!(
            grouped_body("me@example.com", &messages.iter().collect::<Vec<_>>()),
            "me@example.com received messages from Sender 1, Sender 2, Sender 3, Sender 4, Sender 5 (2 others)"
        );
        let mut repeated = messages.iter().collect::<Vec<_>>();
        repeated.push(&messages[6]);
        assert!(grouped_body("me@example.com", &repeated).ends_with("(2 others)"));
    }

    #[test]
    fn falls_back_to_address_or_unknown_sender_when_name_is_missing() {
        let messages = [message("alice@example.com"), message("")];
        assert_eq!(
            grouped_body("me@example.com", &messages.iter().collect::<Vec<_>>()),
            "me@example.com received messages from alice@example.com, Unknown Sender"
        );
    }

    #[test]
    #[ignore = "requires a private graphical session and notification service fixture"]
    fn notification_buttons_dispatch_without_opening_a_window() {
        use gtk::glib;
        use std::{cell::RefCell, rc::Rc};

        gtk::init().unwrap();
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).unwrap();
        let interface = gio::DBusNodeInfo::for_xml(
            "<node><interface name='org.gtk.Notifications'>
             <method name='AddNotification'><arg type='s' direction='in'/>
             <arg type='s' direction='in'/><arg type='a{sv}' direction='in'/></method>
             <method name='RemoveNotification'><arg type='s' direction='in'/>
             <arg type='s' direction='in'/></method></interface></node>",
        )
        .unwrap()
        .lookup_interface("org.gtk.Notifications")
        .unwrap();
        let received = Rc::new(RefCell::new(Vec::new()));
        let payloads = received.clone();
        let registration = connection
            .register_object("/org/gtk/Notifications", &interface)
            .method_call(move |_, _, _, _, method, parameters, invocation| {
                if method == "AddNotification" {
                    payloads.borrow_mut().push(parameters.child_value(2));
                }
                invocation.return_value(None);
            })
            .build()
            .unwrap();
        connection
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "RequestName",
                Some(&("org.gtk.Notifications", 4u32).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                1000,
                gio::Cancellable::NONE,
            )
            .unwrap();
        let app = adw::Application::builder()
            .application_id("io.github.ExposedCat.Brevlada.NotificationActionsDiagnostic")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let (worker, commands) = crate::backend::worker::Worker::recording();
        let engine = Rc::new(super::super::Engine {
            worker: Rc::new(worker),
            listener: RefCell::default(),
            timer: RefCell::default(),
            notified: RefCell::default(),
            accounts: RefCell::default(),
            app: app.downgrade(),
        });
        install_actions(&app, &engine);
        let feedback = Rc::new(RefCell::new(Vec::new()));
        let recorded = feedback.clone();
        engine.attach(move |event| recorded.borrow_mut().push(event));
        let messages: Vec<_> = (1..=6)
            .map(|uid| Message {
                uid,
                ..message(if uid == 2 {
                    "Unknown <unknown@example.com>"
                } else {
                    "Alice <ALICE@example.com>"
                })
            })
            .collect();
        let storage =
            crate::backend::storage::Storage::open(std::path::Path::new(":memory:")).unwrap();
        let square =
            gtk::gdk_pixbuf::Pixbuf::new(gtk::gdk_pixbuf::Colorspace::Rgb, false, 8, 128, 128)
                .unwrap();
        square.fill(0x4285f4ff);
        let image = square.save_to_bufferv("png", &[]).unwrap();
        storage
            .store_avatar(
                "alice@example.com",
                crate::backend::avatars::Source::Google,
                Some(&image),
            )
            .unwrap();
        let icons = storage.notification_icons(&messages);
        let standalone = deliveries("me@example.com", 7, &[&messages[0], &messages[1]]);
        let grouped = deliveries("me@example.com", 7, &messages.iter().collect::<Vec<_>>());
        for delivery in [&standalone[0], &standalone[1], &grouped[0]] {
            app.send_notification(
                Some(&delivery.id),
                &delivery.notification("me@example.com", 7, &icons),
            );
        }
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while received.borrow().len() < 3 && std::time::Instant::now() < deadline {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let payloads = received.borrow();
        assert_eq!(
            payloads.len(),
            3,
            "notification service did not receive all alerts"
        );
        let individual = glib::VariantDict::new(Some(&payloads[0]));
        let sender_icon = gio::Icon::deserialize(&individual.lookup_value("icon", None).unwrap())
            .unwrap()
            .downcast::<gio::BytesIcon>()
            .unwrap();
        let rounded =
            gtk::gdk_pixbuf::Pixbuf::from_read(std::io::Cursor::new(sender_icon.bytes())).unwrap();
        assert_eq!((rounded.width(), rounded.height()), (128, 128));
        assert!(rounded.has_alpha());
        let pixels = rounded.read_pixel_bytes();
        let alpha = |x: usize, y: usize| pixels[y * rounded.rowstride() as usize + x * 4 + 3];
        assert_eq!(alpha(0, 0), 0);
        assert_eq!(alpha(127, 127), 0);
        assert_eq!(alpha(64, 64), 255);
        assert_eq!(alpha(32, 0), 255); // Straight top edge: rounded rectangle, not a circle.
        assert_eq!(alpha(0, 32), 255);
        assert!((0..128).any(|x| (0..128).any(|y| (1..255).contains(&alpha(x, y)))));
        assert_eq!(
            &pixels[64 * rounded.rowstride() as usize + 64 * 4..][..3],
            &[0x42, 0x85, 0xf4]
        );
        for payload in &payloads[1..] {
            let fallback = glib::VariantDict::new(Some(payload));
            let app_icon = gio::Icon::deserialize(&fallback.lookup_value("icon", None).unwrap())
                .unwrap()
                .downcast::<gio::ThemedIcon>()
                .unwrap();
            assert!(
                app_icon
                    .names()
                    .iter()
                    .any(|name| name == "io.github.ExposedCat.Brevlada-symbolic")
            );
        }
        assert_eq!(
            glib::VariantDict::new(Some(&payloads[1]))
                .lookup_value("buttons", None)
                .unwrap()
                .n_children(),
            3
        );
        let buttons = individual.lookup_value("buttons", None).unwrap();
        assert_eq!(buttons.n_children(), 3);
        let target = ("me@example.com", 7u32, 1u32).to_variant();
        assert_eq!(
            individual.lookup_value("default-action-target", None),
            Some(target.clone())
        );
        for (index, (name, action)) in ACTIONS.into_iter().enumerate() {
            let button = glib::VariantDict::new(Some(&buttons.child_value(index)));
            assert_eq!(
                button.lookup::<String>("label").unwrap().unwrap(),
                action.title(false)
            );
            let detailed = button.lookup::<String>("action").unwrap().unwrap();
            assert_eq!(detailed, format!("app.{name}"));
            let actual_target = button.lookup_value("target", None).unwrap();
            assert_eq!(actual_target, target);
            app.activate_action(detailed.strip_prefix("app.").unwrap(), Some(&actual_target));
            let super::super::Command::NotificationAction {
                account,
                validity,
                uid,
                action: actual,
            } = commands.try_recv().unwrap()
            else {
                panic!("Expected a notification action")
            };
            assert_eq!((account.as_str(), validity, uid), ("me@example.com", 7, 1));
            assert_eq!(actual, action);
            let super::super::Event::NotificationActionStarted {
                account,
                validity,
                uid,
                action: started,
            } = feedback.borrow_mut().pop().unwrap()
            else {
                panic!("Expected immediate UI feedback before the worker responds")
            };
            assert_eq!((account.as_str(), validity, uid), ("me@example.com", 7, 1));
            assert_eq!(started, action);
        }
        engine.event(super::super::Event::NotificationActionFinished(
            "me@example.com".into(),
            7,
            1,
            Some("Connection failed".into()),
        ));
        assert!(matches!(
            feedback.borrow_mut().pop(),
            Some(super::super::Event::NotificationActionFinished(
                _,
                7,
                1,
                Some(_)
            ))
        ));
        assert!(
            glib::VariantDict::new(Some(&payloads[2]))
                .lookup_value("buttons", None)
                .is_none()
        );
        assert!(app.windows().is_empty());
        assert!(commands.try_recv().is_err());
        connection.unregister_object(registration).unwrap();
    }
}
