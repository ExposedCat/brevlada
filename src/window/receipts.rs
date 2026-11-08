use super::*;

impl State {
    pub(super) fn receipt_widgets(
        &self,
        messages: &[(bool, String, Message)],
    ) -> Vec<Vec<gtk::Widget>> {
        let mut widgets = vec![Vec::new(); messages.len()];
        let mut latest = HashMap::new();
        for (index, (_, _, message)) in messages
            .iter()
            .enumerate()
            .filter(|(_, (_, _, message))| !message.is_draft)
        {
            let (name, address) = models::senders::identity(message);
            let sender = if name.is_empty() { address } else { name };
            for receipt in &message.receipts {
                latest
                    .entry((
                        models::senders::key(message),
                        receipt.invoice,
                        receipt.number.clone(),
                    ))
                    .or_insert_with(|| (index, receipt, sender.clone()));
            }
        }
        let mut receipts: Vec<_> = latest.into_values().collect();
        receipts.sort_by(|left, right| (left.0, &left.1.number).cmp(&(right.0, &right.1.number)));
        for (index, receipt, sender) in receipts {
            widgets[index].push(ui::receipt::card(receipt, &sender).upcast());
        }
        widgets
    }
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    #[test]
    #[ignore = "Requires a private graphical session and BREVLADA_DASHBOARD_DB copied cache; uses a recording worker"]
    fn xzone_order_uses_the_same_card_in_the_dashboard_and_email() {
        let path = std::env::var("BREVLADA_DASHBOARD_DB").unwrap();
        let connection =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let html: String = connection
            .query_row(
                "SELECT b.body_html FROM rust_headers h JOIN rust_bodies b ON b.id=h.id
             WHERE lower(h.sender) LIKE '%xzone%' AND h.subject LIKE 'Potvrzujeme%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut message = Message {
            uid: 1,
            sender: "Xzone.cz <shop@example.com>".into(),
            message_id: "xzone-fixture".into(),
            subject: "Order confirmation".into(),
            body_loaded: true,
            is_read: true,
            body_html: html,
            ..Default::default()
        };
        message.prepare_display();
        assert_eq!(message.receipts.len(), 1);
        assert!(!message.receipts[0].items.is_empty());
        message.body_html.clear();
        message.body_loaded = false;
        let (worker, _commands) = worker::Worker::recording();
        let state = super::super::diagnostics::unread_test_state_with_worker(
            "org.example.BrevladaOrderCardDiagnostic",
            worker,
        );
        state.show_thread(vec![message.clone()]);
        let card = state.viewer.first_child().unwrap();
        assert!(card.has_css_class("receipt-card"));
        let source = models::dashboard::Source {
            account: "account@example.com".into(),
            folder: "INBOX".into(),
            message: message.clone(),
        };
        let entry = models::dashboard::Entry {
            recent_emails: 0,
            id: "order".into(),
            source,
            content: models::dashboard::Content::Receipt(message.receipts[0].clone()),
            series: Vec::new(),
        };
        let tile = ui::dashboard::Tile::new(&entry, true, false);
        assert!(tile.widget.has_css_class("receipt-card"));
        let window = state.toast.root().and_downcast::<gtk::Window>().unwrap();
        window.present();
        let context = glib::MainContext::default();
        context.block_on(glib::timeout_future(Duration::from_millis(300)));
        if let Ok(path) = std::env::var("BREVLADA_ORDER_SCREENSHOT") {
            let snapshot = gtk::Snapshot::new();
            gtk::WidgetPaintable::new(Some(&window)).snapshot(
                &snapshot,
                window.width() as f64,
                window.height() as f64,
            );
            window
                .renderer()
                .unwrap()
                .render_texture(snapshot.to_node().unwrap(), None)
                .save_to_png(path)
                .unwrap();
        }
        window.close();
    }
}
