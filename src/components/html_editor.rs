use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use webkit6::prelude::*;

const WORLD: &str = "brevlada-compose";

#[derive(Clone)]
pub struct Editor {
    pub view: webkit6::WebView,
    pending: Rc<RefCell<serde_json::Value>>,
    ready: Rc<Cell<bool>>,
}

impl Editor {
    pub fn new(buffer: &gtk::TextBuffer, mode: &gtk::ToggleButton) -> Self {
        let manager = webkit6::UserContentManager::new();
        let settings = webkit6::Settings::builder()
            .enable_javascript(true)
            .enable_javascript_markup(false)
            .enable_html5_local_storage(false)
            .build();
        let view = webkit6::WebView::builder()
            .settings(&settings)
            .user_content_manager(&manager)
            .height_request(crate::theme::COMPOSE_HEIGHT)
            .hexpand(true)
            .build();
        manager.register_script_message_handler("composeText", Some(WORLD));
        let target = buffer.downgrade();
        let mode = mode.downgrade();
        manager.connect_script_message_received(Some("composeText"), move |_, value| {
            if mode.upgrade().is_some_and(|mode| mode.is_active())
                && let Some(buffer) = target.upgrade()
            {
                buffer.set_text(&value.to_str());
            }
        });
        let editor = Self {
            view,
            pending: Rc::new(RefCell::new(serde_json::json!({"html": ""}))),
            ready: Rc::new(Cell::new(false)),
        };
        let ready = editor.ready.clone();
        let pending = editor.pending.clone();
        editor.view.connect_load_changed(move |view, event| {
            if event == webkit6::LoadEvent::Finished {
                ready.set(true);
                run(
                    view,
                    &format!(
                        "const quoteStyle = {};\n{}",
                        serde_json::to_string(include_str!("html_editor/quote.css")).unwrap(),
                        include_str!("html_editor/editor.js")
                    ),
                );
                set_content(view, &pending.borrow());
            }
        });
        editor.view.connect_decide_policy(|_, decision, kind| {
            if matches!(
                kind,
                webkit6::PolicyDecisionType::NavigationAction
                    | webkit6::PolicyDecisionType::NewWindowAction
            ) && let Some(navigation) =
                decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
                && let Some(action) = navigation.navigation_action()
                && action.navigation_type() != webkit6::NavigationType::Other
            {
                decision.ignore();
                return true;
            }
            false
        });
        editor.view.load_html(&format!(
            "<!doctype html><html><head><meta charset='utf-8'><meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:; frame-src 'self';\"><style>{}</style></head><body contenteditable='true' aria-label='Message body'></body></html>",
            include_str!("html_editor/style.css")
        ), Some("about:blank"));
        editor
    }

    pub fn set_html(&self, html: &str) {
        self.set_content(serde_json::json!({"html": html}));
    }

    pub fn reply(&self, html: &str) {
        self.set_content(
            serde_json::json!({"html": "<div><br></div><div><br></div>", "quote": html}),
        );
    }

    fn set_content(&self, content: serde_json::Value) {
        *self.pending.borrow_mut() = content;
        if self.ready.get() {
            set_content(&self.view, &self.pending.borrow());
        }
    }

    pub fn focus(&self) {
        self.view.grab_focus();
        if self.ready.get() {
            run(&self.view, "document.body.focus({preventScroll: true})");
        }
    }
}

fn set_content(view: &webkit6::WebView, content: &serde_json::Value) {
    run(
        view,
        &format!("setContent({})", serde_json::to_string(content).unwrap()),
    );
}

fn run(view: &webkit6::WebView, script: &str) {
    view.evaluate_javascript(
        script,
        Some(WORLD),
        None,
        gtk::gio::Cancellable::NONE,
        |result| {
            if let Err(error) = result {
                gtk::glib::g_warning!("brevlada", "HTML editor: {error}");
            }
        },
    );
}

pub fn toolbar(editor: &Editor, mode: &gtk::ToggleButton) -> gtk::Box {
    let toolbar = super::horizontal("compose-formatting", 0);
    toolbar.add_css_class("linked");
    toolbar.set_visible(mode.is_active());
    for (icon, title, command) in [
        ("format-text-bold-symbolic", "Bold (Ctrl+B)", "Bold"),
        ("format-text-italic-symbolic", "Italic (Ctrl+I)", "Italic"),
        (
            "format-text-underline-symbolic",
            "Underline (Ctrl+U)",
            "Underline",
        ),
        (
            "format-text-strikethrough-symbolic",
            "Strikethrough (Ctrl+Shift+X)",
            "Strikethrough",
        ),
        (
            "edit-clear-symbolic",
            "Clear formatting (Ctrl+\\)",
            "RemoveFormat",
        ),
    ] {
        let button = super::button(icon, title);
        button.remove_css_class("flat");
        button.set_focus_on_click(false);
        let target = editor.view.downgrade();
        button.connect_clicked(move |_| {
            if let Some(view) = target.upgrade() {
                view.execute_editing_command(command);
                view.grab_focus();
            }
        });
        toolbar.append(&button);
    }
    let target = toolbar.downgrade();
    mode.connect_toggled(move |mode| {
        if let Some(toolbar) = target.upgrade() {
            toolbar.set_visible(mode.is_active());
        }
    });
    toolbar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session; presents an isolated editor without mail workers"]
    fn renders_and_edits_html_quotes() {
        gtk::init().unwrap();
        let buffer = gtk::TextBuffer::new(None);
        let mode = gtk::ToggleButton::new();
        mode.set_active(true);
        let editor = Editor::new(&buffer, &mode);
        editor.reply("<!doctype html><html><head><style>.heading { background: rgb(0, 120, 180); color: white; } td { height: 800px; }</style></head><body bgcolor='#f6f6f6'><p class='heading'><b>Original</b></p><table><tr><td style='color: rgb(255, 0, 0)'>Cell</td></tr></table><script>document.body.textContent='bad'</script></body></html>");
        let window = gtk::Window::builder().child(&editor.view).build();
        window.present();
        let context = gtk::glib::MainContext::default();
        context.block_on(async {
            for _ in 0..250 {
                if editor.ready.get() {
                    break;
                }
                gtk::glib::timeout_future(std::time::Duration::from_millis(20)).await;
            }
            assert!(editor.ready.get(), "HTML editor did not load");
            gtk::glib::timeout_future(std::time::Duration::from_millis(300)).await;
            let result = editor.view.evaluate_javascript_future(
                "const quoteDoc = document.querySelector('iframe').contentDocument; JSON.stringify({background: getComputedStyle(quoteDoc.querySelector('.heading')).backgroundColor, bodyBackground: getComputedStyle(quoteDoc.body).backgroundColor, scroll: window.scrollY, frameHeight: document.querySelector('iframe').height, quote: quoteDoc.querySelector('b').textContent, table: quoteDoc.querySelectorAll('td').length, color: getComputedStyle(quoteDoc.querySelector('td')).color, scripts: quoteDoc.querySelectorAll('script').length, top: getSelection().anchorNode === document.body.firstChild, offset: getSelection().anchorOffset})",
                Some(WORLD), None,
            ).await.unwrap();
            let result: serde_json::Value = serde_json::from_str(&result.to_str()).unwrap();
            assert_eq!(result["background"], "rgb(0, 120, 180)");
            assert_eq!(result["bodyBackground"], "rgb(246, 246, 246)");
            assert_eq!(result["scroll"], 0);
            assert!(result["frameHeight"].as_str().unwrap().parse::<i32>().unwrap() >= 800);
            assert_eq!(result["quote"], "Original");
            assert_eq!(result["table"], 1);
            assert_eq!(result["color"], "rgb(255, 0, 0)");
            assert_eq!(result["scripts"], 0);
            assert_eq!(result["top"], true);
            assert_eq!(result["offset"], 0);
            editor.view.evaluate_javascript_future(
                "document.execCommand('insertText', false, 'My reply'); report()",
                Some(WORLD), None,
            ).await.unwrap();
            gtk::glib::timeout_future(std::time::Duration::from_millis(20)).await;
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
            assert!(text.starts_with("My reply\n\n"), "{text:?}");
            assert!(text.contains("Original"));
        });
        editor.view.stop_loading();
        window.destroy();
    }
}
