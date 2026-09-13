use super::html_editor::{Editor, WORLD, run};
use adw::prelude::*;
use webkit6::prelude::*;

pub fn toolbar(editor: &Editor, mode: &gtk::ToggleButton) -> gtk::Box {
    let toolbar = super::horizontal("compose-formatting", 0);
    toolbar.add_css_class("linked");
    toolbar.set_visible(mode.is_active());
    let mut toggles = Vec::new();
    for (icon, title, command) in [
        ("format-text-bold-symbolic", "Bold (Ctrl+B)", "bold"),
        ("format-text-italic-symbolic", "Italic (Ctrl+I)", "italic"),
        (
            "format-text-underline-symbolic",
            "Underline (Ctrl+U)",
            "underline",
        ),
        (
            "format-text-strikethrough-symbolic",
            "Strikethrough (Ctrl+Shift+X)",
            "strikeThrough",
        ),
    ] {
        let toggle = gtk::ToggleButton::builder()
            .icon_name(icon)
            .tooltip_text(title)
            .focus_on_click(false)
            .build();
        connect(&toggle, &editor.view, command);
        toggles.push(toggle.downgrade());
        toolbar.append(&toggle);
    }
    let clear = super::button("edit-clear-symbolic", "Clear formatting (Ctrl+\\)");
    clear.remove_css_class("flat");
    clear.set_focus_on_click(false);
    connect(&clear, &editor.view, "removeFormat");
    toolbar.append(&clear);
    let manager = editor.view.user_content_manager().unwrap();
    manager.register_script_message_handler("composeFormats", Some(WORLD));
    manager.connect_script_message_received(Some("composeFormats"), move |_, value| {
        if let Ok(active) = serde_json::from_str::<[bool; 4]>(&value.to_str()) {
            for (toggle, active) in toggles.iter().zip(active) {
                if let Some(toggle) = toggle.upgrade() {
                    toggle.set_active(active);
                }
            }
        }
    });
    let target = toolbar.downgrade();
    let view = editor.view.downgrade();
    mode.connect_toggled(move |mode| {
        if let Some(toolbar) = target.upgrade() {
            toolbar.set_visible(mode.is_active());
        }
        if mode.is_active()
            && let Some(view) = view.upgrade()
            && !view.is_loading()
        {
            run(&view, "reportFormats()");
        }
    });
    toolbar
}

fn connect(button: &impl IsA<gtk::Button>, view: &webkit6::WebView, command: &'static str) {
    let target = view.downgrade();
    button.connect_clicked(move |_| {
        if let Some(view) = target.upgrade() {
            view.grab_focus();
            run(
                &view,
                &format!("applyFormat({})", serde_json::to_string(command).unwrap()),
            );
        }
    });
}
