use super::{column, label};
use crate::{
    models::{receipt::Receipt, ticket},
    theme,
};
use adw::prelude::*;

pub fn card(receipt: &Receipt, sender: &str) -> gtk::Box {
    with_actions(receipt, sender).0
}

pub fn with_actions(receipt: &Receipt, sender: &str) -> (gtk::Box, gtk::FlowBox) {
    let widget = column("receipt-card");
    widget.set_spacing(theme::SPACING);
    widget.append(&super::card_title(
        "money-symbolic",
        receipt.merchant.as_deref().unwrap_or(sender),
    ));
    widget.append(&text(&receipt.amount, "title-1"));
    widget.append(&text(
        &format!(
            "{} {}",
            if receipt.invoice { "Invoice" } else { "Order" },
            receipt.number
        ),
        "dim-label",
    ));
    if let Some(date) = &receipt.date {
        widget.append(&super::card_detail(
            "x-office-calendar-symbolic",
            &ticket::date_label(date),
        ));
    }
    for item in &receipt.items {
        widget.append(&super::card_detail("package-x-generic-symbolic", item));
    }
    let actions = super::card_actions();
    if let Some(url) = &receipt.url {
        let link = super::action_button(
            "web-browser-symbolic",
            if receipt.invoice {
                "View invoice"
            } else {
                "View order"
            },
        );
        let url = url.clone();
        link.connect_clicked(move |_| super::links::open(&url));
        super::append_action(&actions, &link);
    }
    widget.append(&actions);
    (widget, actions)
}

fn text(value: &str, class: &str) -> gtk::Label {
    let label = label(value, class);
    label.set_selectable(true);
    label.set_halign(gtk::Align::Fill);
    label.set_wrap(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::None);
    label
}

#[cfg(test)]
mod diagnostics {
    use super::*;

    fn descendants(widget: &gtk::Widget, output: &mut Vec<gtk::Widget>) {
        output.push(widget.clone());
        let mut child = widget.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            descendants(&widget, output);
        }
    }

    #[test]
    #[ignore = "Requires a private graphical session; copies fixture card text on the private clipboard"]
    fn card_icons_follow_titles_and_card_values_can_be_selected_and_copied() {
        gtk::init().unwrap();
        adw::init().unwrap();
        gtk::gio::resources_register_include!("brevlada.gresource").unwrap();
        let display = gtk::gdk::Display::default().unwrap();
        let icons = gtk::IconTheme::for_display(&display);
        icons.add_resource_path("/org/gtk/example/icons");
        for icon in [
            "view-pin-symbolic",
            "money-symbolic",
            "package-x-generic-symbolic",
        ] {
            assert!(icons.has_icon(icon), "Missing {icon}");
        }
        let css = gtk::CssProvider::new();
        css.load_from_string(theme::CSS);
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let content = column("card-copy-diagnostic");
        content.set_spacing(theme::SPACING);
        content.append(&card(
            &Receipt {
                number: "1112329699".into(),
                merchant: Some("Xzone.cz".into()),
                amount: "CZK 558".into(),
                date: Some("2026-06-09".into()),
                invoice: false,
                url: None,
                items: vec!["Pokladnička Pusheen - Package".into()],
            },
            "Shop",
        ));
        content.append(&super::super::calendar::card(
            &crate::models::calendar::Event {
                name: "Psychiatry".into(),
                start: "2027-01-25T09:30:00+01:00".into(),
                location: Some("Unicare Medical, Prague 6".into()),
                organizer: Some("Clinic".into()),
                ..Default::default()
            },
            "fixture@example.com",
            false,
            |_| {},
        ));
        let window = gtk::Window::builder()
            .child(&content)
            .default_width(700)
            .build();
        window.present();
        let context = gtk::glib::MainContext::default();
        context.block_on(gtk::glib::timeout_future(std::time::Duration::from_millis(
            200,
        )));
        let mut widgets = Vec::new();
        descendants(content.upcast_ref(), &mut widgets);
        for value in [
            "Xzone.cz",
            "CZK 558",
            "Order 1112329699",
            "Pokladnička Pusheen - Package",
            "Psychiatry",
            "Unicare Medical, Prague 6",
            "Organized by Clinic",
        ] {
            let label = widgets
                .iter()
                .filter_map(|widget| widget.downcast_ref::<gtk::Label>())
                .find(|label| label.text() == value)
                .unwrap();
            assert!(label.is_selectable(), "Not selectable: {value}");
            label.select_region(0, -1);
            label.emit_copy_clipboard();
            let copied = context
                .block_on(display.clipboard().read_text_future())
                .unwrap()
                .unwrap();
            assert_eq!(copied, value);
        }
        for header in widgets
            .iter()
            .filter(|widget| widget.has_css_class("card-title"))
        {
            let image = header.first_child().and_downcast::<gtk::Image>().unwrap();
            let title = header.last_child().and_downcast::<gtk::Label>().unwrap();
            assert!(image.height() as f32 >= title.height() as f32 * 0.7);
            assert!(image.height() <= title.height());
        }
        for (class, icon) in [
            ("receipt-card", "money-symbolic"),
            ("calendar-card", "view-pin-symbolic"),
        ] {
            let card = widgets
                .iter()
                .find(|widget| widget.has_css_class(class))
                .unwrap();
            let image = card
                .first_child()
                .unwrap()
                .first_child()
                .and_downcast::<gtk::Image>()
                .unwrap();
            assert_eq!(image.icon_name().as_deref(), Some(icon));
        }
        assert!(
            widgets
                .iter()
                .filter_map(|widget| widget.downcast_ref::<gtk::Image>())
                .any(|image| image.icon_name().as_deref() == Some("package-x-generic-symbolic"))
        );
        if let Ok(path) = std::env::var("BREVLADA_CARD_ICONS_SCREENSHOT") {
            for label in widgets
                .iter()
                .filter_map(|widget| widget.downcast_ref::<gtk::Label>())
            {
                label.select_region(0, 0);
            }
            GtkWindowExt::set_focus(&window, gtk::Widget::NONE);
            context.block_on(gtk::glib::timeout_future(std::time::Duration::from_millis(
                200,
            )));
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
