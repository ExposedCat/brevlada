use adw::prelude::*;
use gtk::glib;

pub fn follow_ui(view: &webkit6::WebView) {
    update(view);
    view.connect_map(update);
    for property in ["gtk-font-name", "gtk-xft-dpi"] {
        gtk::prelude::WidgetExt::settings(view).connect_closure(
            &format!("notify::{property}"),
            false,
            gtk::glib::closure_local!(
                #[weak]
                view,
                move |_: gtk::Settings, _: gtk::glib::ParamSpec| update(&view)
            ),
        );
    }
}

fn update(view: &webkit6::WebView) {
    let context = view.pango_context();
    let description = context.font_description().expect("GTK provides a UI font");
    let font = context
        .load_font(&description)
        .expect("GTK loads its UI font");
    let pixels = (f64::from(font.describe_with_absolute_size().size())
        / f64::from(gtk::pango::SCALE))
    .round() as u32;
    let settings =
        webkit6::prelude::WebViewExt::settings(view).expect("Message views have WebKit settings");
    settings.set_default_font_size(pixels);
    settings.set_default_monospace_font_size(pixels);
    settings.set_minimum_font_size(pixels);
    if let Some(family) = description.family() {
        settings.set_default_font_family(&family);
        settings.set_sans_serif_font_family(&family);
    }
}
