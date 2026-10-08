use super::*;

fn entry(compose: &ui::compose::Compose, field: &str) -> gtk::Entry {
    let header = compose
        .widget
        .first_child()
        .unwrap()
        .next_sibling()
        .unwrap();
    match field {
        "to" => header.first_child().unwrap(),
        "cc" => header.first_child().unwrap().next_sibling().unwrap(),
        "subject" => header.next_sibling().unwrap(),
        _ => unreachable!(),
    }
    .downcast()
    .unwrap()
}

fn button(widget: &gtk::Widget, tooltip: &str) -> Option<gtk::Button> {
    if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && button.tooltip_text().as_deref() == Some(tooltip)
    {
        return Some(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(button) = button(&widget, tooltip) {
            return Some(button);
        }
        child = widget.next_sibling();
    }
    None
}

#[test]
#[ignore = "Requires a graphical session for independent response composers"]
fn reply_all_and_forward_keep_independent_edits_and_original_account() {
    gtk::init().unwrap();
    adw::init().unwrap();
    let app = adw::Application::builder()
        .application_id("org.example.BrevladaResponseDraftDiagnostic")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gtk::gio::Cancellable::NONE).unwrap();
    let shell = ui::shell::Shell::new(&app);
    let window = shell.window.clone();
    let (worker, commands) = worker::Worker::recording();
    let state = State::new(
        shell,
        worker,
        ui::expansion::Expansion::default(),
        ui::avatars::Avatars::new(|_| {}),
    );
    *state.account.borrow_mut() = Some(Account {
        email: "me@example.com".into(),
        path: String::new(),
        name: String::new(),
        host: String::new(),
        username: String::new(),
        port: 993,
        ssl: true,
        tls: false,
        oauth2: false,
        smtp: None,
    });
    *state.folder.borrow_mut() = "INBOX".into();
    let incoming = Message {
        uid: 42,
        uid_validity: Some(7),
        message_id: "incoming".into(),
        sender: "sender@example.com".into(),
        reply_to: "support@example.com".into(),
        recipients: "me@example.com, team@example.com".into(),
        cc: "copy@example.com".into(),
        subject: "Topic".into(),
        body_text: "Original body".into(),
        attachments: vec!["report.pdf".into()],
        body_loaded: true,
        is_read: true,
        ..Default::default()
    };
    let card = state.card(&incoming, true, None, false, false);
    let mut composers = Vec::new();
    for action in ["Reply", "Reply All", "Forward", "Reply All"] {
        button(card.widget.upcast_ref(), action)
            .unwrap()
            .emit_clicked();
        let compose = state
            .open_composers
            .borrow()
            .last()
            .unwrap()
            .compose
            .clone();
        assert_eq!(
            entry(&compose, "subject").text(),
            if action == "Forward" {
                "Fwd: Topic"
            } else {
                "Re: Topic"
            }
        );
        entry(&compose, "subject").set_text(&format!("Edited {action}"));
        composers.push(compose);
    }
    assert_eq!(state.open_composers.borrow().len(), 4);
    assert_eq!(entry(&composers[0], "subject").text(), "Edited Reply");
    assert_eq!(entry(&composers[1], "subject").text(), "Edited Reply All");
    assert_eq!(entry(&composers[2], "subject").text(), "Edited Forward");
    assert_eq!(
        entry(&composers[1], "to").text(),
        "support@example.com, team@example.com"
    );
    assert_eq!(entry(&composers[1], "cc").text(), "copy@example.com");
    assert!(entry(&composers[2], "to").text().is_empty());
    entry(&composers[2], "to").set_text("forward@example.com");

    state.account.borrow_mut().as_mut().unwrap().email = "another@example.com".into();
    *state.folder.borrow_mut() = "Other folder".into();
    state.generation.set(state.generation.get() + 1);
    state.selection.set(state.selection.get() + 1);
    for compose in &composers[1..3] {
        button(compose.widget.upcast_ref(), "Send")
            .unwrap()
            .emit_clicked();
        let context = glib::MainContext::default();
        while context.pending() {
            context.iteration(false);
        }
        let Command::Compose {
            account,
            draft,
            target,
            ..
        } = commands.try_recv().unwrap()
        else {
            panic!("Expected send");
        };
        assert_eq!(account.email, "me@example.com");
        assert!(target.is_none());
        if draft.subject == "Edited Reply All" {
            assert_eq!(draft.to, "support@example.com, team@example.com");
            assert_eq!(draft.cc, "copy@example.com");
            assert_eq!(draft.in_reply_to.as_deref(), Some("incoming"));
        } else {
            assert_eq!(draft.subject, "Edited Forward");
            assert_eq!(draft.to, "forward@example.com");
            assert!(draft.in_reply_to.is_none());
            let source = draft.attachment_source.unwrap();
            assert_eq!(source.folder, "INBOX");
            assert_eq!(source.uid, 42);
        }
    }
    assert!(commands.try_recv().is_err());
    assert_eq!(entry(&composers[0], "subject").text(), "Edited Reply");
    assert_eq!(entry(&composers[3], "subject").text(), "Edited Reply All");
    window.close();
}
