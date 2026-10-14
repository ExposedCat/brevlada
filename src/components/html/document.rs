use scraper::{Html, Selector};

#[derive(Default)]
pub(super) struct Document {
    pub styles: String,
    pub attributes: String,
    pub content: String,
}

pub(super) fn parse(source: &str) -> Document {
    let html = Html::parse_document(source);
    let mut document = Document {
        styles: html
            .select(&Selector::parse("head style").unwrap())
            .map(|style| {
                let mut result = String::new();
                super::serialize(&mut result, *style, true);
                result
            })
            .collect(),
        ..Default::default()
    };
    if let Some(body) = html.select(&Selector::parse("body").unwrap()).next() {
        for (name, value) in body.value().attrs() {
            if matches!(
                name,
                "style"
                    | "class"
                    | "id"
                    | "dir"
                    | "lang"
                    | "bgcolor"
                    | "background"
                    | "text"
                    | "link"
                    | "vlink"
                    | "alink"
            ) {
                document.attributes.push_str(&format!(
                    " {name}=\"{}\"",
                    gtk::glib::markup_escape_text(value)
                ));
            }
        }
        super::serialize(&mut document.content, *body, false);
    }
    document
}

#[cfg(test)]
mod diagnostics {
    use adw::prelude::*;
    use webkit6::prelude::*;

    #[test]
    #[ignore = "Requires a graphical session; isolated message views without mail workers"]
    fn spacing_preserves_authored_layouts_and_quoted_content() {
        adw::init().unwrap();
        let context = gtk::glib::MainContext::default();
        for (source, padding, margin, background) in [
            ("<div>Hello</div>", 0, 0, "rgba(0, 0, 0, 0)"),
            (
                "<div style='display:flex;flex-wrap:wrap;align-items:center'><img width='20' height='20' src='cid:avatar'><strong>Sender</strong> left a comment <a href='https://example.com'>online</a></div><p>Hello</p>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<p style='margin:0'>Hello</p><img style='border:0; width:1; height:1; display:none' src='https://example.com/pixel'>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<div>Hello</div><blockquote><div>Previous message</div></blockquote>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<div>Hello</div><img width='1' height='1' src='cid:pixel'>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            ("<p>Hello</p><p>Another line</p>", 0, 0, "rgba(0, 0, 0, 0)"),
            (
                "<div style='padding:20px'><p>Hello</p></div><div style='padding:20px'><p>Another line</p></div>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<div style='padding:4px'><p style='margin:0'>Hello</p></div><div style='padding:4px'><p style='margin:0'>Another line</p></div>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<p>Hello</p><table><tr><th>Workload</th><th>Result</th></tr><tr><td>Parsing</td><td>Fast</td></tr></table>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<body style='margin-top:20px'><p>Hello</p></body>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<body style='padding:20px;background:rgb(0,120,180)'><p>Hello</p></body>",
                20,
                0,
                "rgb(0, 120, 180)",
            ),
            (
                "<head><style>body {padding:24px;background:rgb(0,120,180)}</style></head><body><p>Hello</p></body>",
                24,
                0,
                "rgb(0, 120, 180)",
            ),
            (
                "<head><style>body {margin:20px}</style></head><body><p>Hello</p></body>",
                0,
                20,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<body bgcolor='#0078b4'><div>Hello</div></body>",
                0,
                0,
                "rgb(0, 120, 180)",
            ),
            (
                "<table style='width:100%;background:rgb(0,120,180)' cellpadding='20'><tr><td>Hello</td></tr></table>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<div style='padding:20px'><div>Hello</div></div>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<div style='background:rgb(0,120,180)'>Hello</div>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<img style='width:100%' src='cid:banner'><p>Hello</p>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<div data-preserve-layout style='width:100%;background:rgb(0,120,180)'><div style='padding:20px'>Hello</div></div><a href='https://example.com'>unsubscribe</a>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<table data-preserve-layout style='width:100%;background:rgb(0,120,180)' cellpadding='0' cellspacing='0'><tr><td><div style='width:60%;margin:auto;padding:20px'>Hello</div></td></tr></table><a href='https://example.com'>unsubscribe</a>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
            (
                "<div><img data-preserve-layout style='display:block;width:100%;height:20px' src='cid:banner'>Hello</div>",
                0,
                0,
                "rgba(0, 0, 0, 0)",
            ),
        ] {
            let message = crate::models::Message {
                body_html: source.into(),
                body_loaded: true,
                ..Default::default()
            };
            let (widget, view) = crate::components::body::view(&message, false, false);
            let scroll = gtk::ScrolledWindow::builder().child(&widget).build();
            let window = gtk::Window::builder()
                .child(&scroll)
                .default_width(800)
                .default_height(500)
                .build();
            window.present();
            context.block_on(async {
                gtk::glib::timeout_future(std::time::Duration::from_millis(500)).await;
                let value = view.evaluate_javascript_future(
                    "(()=>{const style=getComputedStyle(document.body);const root=document.getElementById('brevlada-content');return JSON.stringify({padding:parseFloat(style.paddingLeft),margin:parseFloat(style.marginLeft),background:style.backgroundColor,bottom:root.getBoundingClientRect().bottom+parseFloat(style.paddingBottom)+parseFloat(style.marginBottom),height:innerHeight, edges:(()=>{const r=[];const w=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);while(w.nextNode()){if(!w.currentNode.textContent.trim())continue;const range=document.createRange();range.selectNodeContents(w.currentNode);for(const b of range.getClientRects())if(b.width&&b.height)r.push(b);}return [Math.min(...r.map(b=>b.left)),Math.min(...r.map(b=>innerWidth-b.right)),Math.min(...r.map(b=>b.top)),document.body.getBoundingClientRect().bottom+parseFloat(style.marginBottom)-Math.max(...r.map(b=>b.bottom))];})()});})()",
                    None, None,
                ).await.unwrap();
                let result: serde_json::Value = serde_json::from_str(&value.to_str()).unwrap();
                assert_eq!(result["padding"], padding, "{source}: {result}");
                assert_eq!(result["margin"], margin, "{source}: {result}");
                assert_eq!(result["background"], background, "{source}: {result}");
                for edge in result["edges"].as_array().unwrap() {
                    assert!(edge.as_f64().unwrap() >= 11.9, "Missing inset: {source}: {result}");
                }
                assert!(result["height"].as_f64().unwrap() >= result["bottom"].as_f64().unwrap(), "Clipped: {source}: {result}");
                let panels = view.evaluate_javascript_future(
                    "JSON.stringify([...document.querySelectorAll('[data-preserve-layout]')].map(panel=>{const b=panel.getBoundingClientRect();return [b.left,innerWidth-b.right,b.top]}))", None, None,
                ).await.unwrap();
                let panels: Vec<Vec<f64>> = serde_json::from_str(&panels.to_str()).unwrap();
                for panel in panels {
                    assert!(panel.iter().all(|edge| edge.abs() < 0.1), "Authored panel shifted: {source}: {panel:?}");
                }
            });
            window.destroy();
        }
        for (source, padding) in [
            ("<div>Hello</div>", 0),
            (
                "<div style='display:flex;align-items:center'><img width='20' height='20' src='cid:avatar'><strong>Sender</strong> left a comment</div><p>Hello</p>",
                0,
            ),
            ("<body style='padding:20px'><div>Hello</div></body>", 20),
            (
                "<table style='width:100%;background:rgb(0,120,180)' cellpadding='20'><tr><td>Hello</td></tr></table>",
                0,
            ),
            (
                "<div style='background:blue;width:100%;padding:20px'>Hello</div><a href='https://example.com'>unsubscribe</a>",
                0,
            ),
        ] {
            let editor = crate::components::html_editor::Editor::new(
                &gtk::TextBuffer::new(None),
                &gtk::ToggleButton::new(),
                |_, _| {},
            );
            editor.reply(source);
            let window = gtk::Window::builder()
                .child(&editor.view)
                .default_width(800)
                .default_height(400)
                .build();
            window.present();
            context.block_on(async {
                editor.message().await.unwrap();
                gtk::glib::timeout_future(std::time::Duration::from_millis(300)).await;
                let (html, _) = editor.message().await.unwrap();
                let value = editor.view.evaluate_javascript_future(
                    "parseFloat(getComputedStyle(document.querySelector('iframe').contentDocument.body).paddingLeft)",
                    None, None,
                ).await.unwrap();
                if padding > 0 {
                    assert_eq!(value.to_double(), f64::from(padding), "{source}");
                }
                let inset = editor.view.evaluate_javascript_future(
                    "(()=>{const doc=document.querySelector('iframe').contentDocument;const r=[];const w=doc.createTreeWalker(doc.body,NodeFilter.SHOW_TEXT);while(w.nextNode()){if(!w.currentNode.textContent.trim())continue;const range=doc.createRange();range.selectNodeContents(w.currentNode);for(const b of range.getClientRects())if(b.width&&b.height)r.push(b.left);}return Math.min(...r);})()",
                    None, None,
                ).await.unwrap();
                assert!(inset.to_double() >= 11.9, "{source}");
                assert!(!html.contains("brevlada-inset"));
                assert!(!html.contains("brevlada-spacing-wrapper"));
                assert!(html.contains("Hello"));
            });
            window.destroy();
        }
        crate::components::body_renderer::clear();
    }
}
