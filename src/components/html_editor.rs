use adw::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use webkit6::prelude::*;

pub(super) const WORLD: &str = "brevlada-compose";

#[derive(Clone)]
pub struct Editor {
    pub view: webkit6::WebView,
    pending: Rc<RefCell<serde_json::Value>>,
    ready: Rc<Cell<bool>>,
}

impl Editor {
    pub fn new(
        buffer: &gtk::TextBuffer,
        mode: &gtk::ToggleButton,
        height_changed: impl Fn(i32, bool) + 'static,
    ) -> Self {
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
            .vexpand(false)
            .hexpand(true)
            .build();
        view.set_background_color(&gtk::gdk::RGBA::TRANSPARENT);
        manager.register_script_message_handler("composeHeight", Some(WORLD));
        manager.connect_script_message_received(Some("composeHeight"), move |_, value| {
            if let Ok((height, edited)) = serde_json::from_str::<(i32, bool)>(&value.to_str()) {
                height_changed(height, edited);
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_name(Some("compose-html-shortcuts"));
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let target = view.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let modifiers = modifiers & gtk::accelerator_get_default_mod_mask();
            let control = gtk::gdk::ModifierType::CONTROL_MASK;
            let key = key.to_lower();
            let script = if modifiers == control {
                match key {
                    gtk::gdk::Key::a => Some("selectAllContent()"),
                    gtk::gdk::Key::b => Some("applyFormat('bold')"),
                    gtk::gdk::Key::i => Some("applyFormat('italic')"),
                    gtk::gdk::Key::u => Some("applyFormat('underline')"),
                    gtk::gdk::Key::backslash => Some("applyFormat('removeFormat')"),
                    _ => None,
                }
            } else if modifiers == control | gtk::gdk::ModifierType::SHIFT_MASK
                && key == gtk::gdk::Key::x
            {
                Some("applyFormat('strikeThrough')")
            } else {
                None
            };
            if let Some(script) = script {
                if let Some(view) = target.upgrade() {
                    run(&view, script);
                }
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
        view.add_controller(keys);
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
                        concat!(
                            include_str!("html_editor/editor.js"),
                            "\n",
                            include_str!("html_editor/formatting.js")
                        )
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
            "<!doctype html><html><head><meta charset='utf-8'><meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; script-src 'none'; style-src 'unsafe-inline'; img-src data:; frame-src 'self';\"><style>:root {{ --compose-min-height: {}px; }}{}</style></head><body contenteditable='true' aria-label='Message body'></body></html>",
            crate::theme::COMPOSE_HEIGHT,
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

pub(super) fn run(view: &webkit6::WebView, script: &str) {
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

pub use super::html_formatting::toolbar;

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
        let measured = Rc::new(Cell::new(0));
        let grew = Rc::new(Cell::new(false));
        let previous = measured.clone();
        let growth = grew.clone();
        let editor = Editor::new(&buffer, &mode, move |height, edited| {
            let before = previous.replace(height);
            if edited && height > before {
                growth.set(true);
            }
        });
        editor.reply("<!doctype html><html><head><style>.heading { background: rgb(0, 120, 180); color: white; } td { height: 800px; }</style></head><body bgcolor='#f6f6f6'><p class='heading'><b>Original</b></p><table><tr><td style='color: rgb(255, 0, 0)'>Cell</td></tr></table><script>document.body.textContent='bad'</script></body></html>");
        let strip = toolbar(&editor, &mode);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&strip);
        column.append(&editor.view);
        let window = gtk::Window::builder().child(&column).build();
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
                "const quoteDoc = document.querySelector('iframe').contentDocument; JSON.stringify({background: getComputedStyle(quoteDoc.querySelector('.heading')).backgroundColor, bodyBackground: getComputedStyle(quoteDoc.body).backgroundColor, scroll: window.scrollY, frameHeight: document.querySelector('iframe').height, quote: quoteDoc.querySelector('b').textContent, table: quoteDoc.querySelectorAll('td').length, color: getComputedStyle(quoteDoc.querySelector('td')).color, scripts: quoteDoc.querySelectorAll('script').length, top: document.body.firstChild.contains(getSelection().anchorNode) || (getSelection().anchorNode === document.body && getSelection().anchorOffset === 0), offset: getSelection().anchorOffset})",
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
            assert_eq!(result["top"], true, "{result}");
            assert_eq!(result["offset"], 0);
            editor.view.evaluate_javascript_future(
                "document.execCommand('insertText', false, 'My reply'); report()",
                Some(WORLD), None,
            ).await.unwrap();
            gtk::glib::timeout_future(std::time::Duration::from_millis(20)).await;
            let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
            assert!(text.starts_with("My reply\n\n"), "{text:?}");
            assert!(text.contains("Original"));
            editor.view.evaluate_javascript_future("document.execCommand('insertParagraph'); document.execCommand('insertText', false, 'Another line')", Some(WORLD), None).await.unwrap();
            gtk::glib::timeout_future(std::time::Duration::from_millis(30)).await;
            assert!(grew.get(), "Typing a new HTML line must report content growth");
            let controllers = editor.view.observe_controllers();
            let keys = (0..controllers.n_items()).filter_map(|index| controllers.item(index)?.downcast::<gtk::EventControllerKey>().ok()).find(|keys| keys.name().as_deref() == Some("compose-html-shortcuts")).unwrap();
            assert!(keys.emit_by_name::<bool>("key-pressed", &[&gtk::gdk::Key::a, &0u32, &gtk::gdk::ModifierType::CONTROL_MASK]));
            let selection = editor.view.evaluate_javascript_future("JSON.stringify({collapsed: getSelection().isCollapsed, text: getSelection().toString()})", Some(WORLD), None).await.unwrap();
            let selection: serde_json::Value = serde_json::from_str(&selection.to_str()).unwrap();
            assert_eq!(selection["collapsed"], false);
            assert!(selection["text"].as_str().unwrap().contains("My reply"));
            let bold = strip.first_child().unwrap().downcast::<gtk::ToggleButton>().unwrap();
            let italic = bold.next_sibling().unwrap().downcast::<gtk::ToggleButton>().unwrap();
            let clear = strip.last_child().unwrap().downcast::<gtk::Button>().unwrap();
            for newline in [false, true] {
                editor.view.evaluate_javascript_future(
                    "setContent({html: '<div><b><i>Styled</i></b></div>'}); { const range = document.createRange(); range.selectNodeContents(document.querySelector('i')); range.collapse(false); getSelection().removeAllRanges(); getSelection().addRange(range); } reportFormats()",
                    Some(WORLD), None).await.unwrap();
                if newline {
                    editor.view.evaluate_javascript_future("document.execCommand('insertParagraph'); reportFormats()", Some(WORLD), None).await.unwrap();
                }
                gtk::glib::timeout_future(std::time::Duration::from_millis(30)).await;
                assert!(bold.is_active() && italic.is_active());
                clear.emit_clicked();
                editor.view.evaluate_javascript_future("document.execCommand('insertText', false, 'Plain'); reportFormats()", Some(WORLD), None).await.unwrap();
                gtk::glib::timeout_future(std::time::Duration::from_millis(30)).await;
                assert!(!bold.is_active() && !italic.is_active());
                let style = editor.view.evaluate_javascript_future(
                    "(() => { const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT); let styled; while (walker.nextNode()) if (walker.currentNode.data === 'Styled') styled = walker.currentNode; const typed = getComputedStyle(getSelection().anchorNode.parentElement); const original = getComputedStyle(styled.parentElement); return JSON.stringify({weight: typed.fontWeight, style: typed.fontStyle, originalWeight: original.fontWeight, originalStyle: original.fontStyle, html: document.body.innerHTML}); })()",
                    Some(WORLD), None).await.unwrap();
                let style: serde_json::Value = serde_json::from_str(&style.to_str()).unwrap();
                assert_eq!(style["weight"], "400", "{style}");
                assert_eq!(style["style"], "normal", "{style}");
                assert_eq!(style["originalWeight"], "700", "{style}");
                assert_eq!(style["originalStyle"], "italic", "{style}");
                bold.emit_clicked();
                editor.view.evaluate_javascript_future("reportFormats()", Some(WORLD), None).await.unwrap();
                gtk::glib::timeout_future(std::time::Duration::from_millis(30)).await;
                assert!(bold.is_active());
            }
        });
        editor.view.stop_loading();
        window.destroy();
    }
}
