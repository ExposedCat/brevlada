use adw::prelude::*;
use gtk::glib;

mod imp {
    use super::*;
    use glib::subclass::prelude::*;
    use std::cell::RefCell;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::ActiveRow)]
    pub struct ActiveRow {
        #[property(get, set, nullable)]
        key: RefCell<Option<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ActiveRow {
        const NAME: &'static str = "BrevladaActiveRow";
        type Type = super::ActiveRow;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ActiveRow {}
}

glib::wrapper! {
    pub struct ActiveRow(ObjectSubclass<imp::ActiveRow>);
}

impl Default for ActiveRow {
    fn default() -> Self {
        glib::Object::builder().build()
    }
}

impl ActiveRow {
    pub fn bind(&self, row: &impl IsA<gtk::Widget>, key: String) {
        let classes = row.css_classes();
        self.property_expression("key")
            .chain_closure_with_callback(move |values| {
                let active = values[1].get::<Option<String>>().unwrap();
                let mut classes = classes.clone();
                if active.as_deref() == Some(key.as_str()) {
                    classes.push("message-row-open".into());
                }
                classes
            })
            .bind(row.as_ref(), "css-classes", gtk::Widget::NONE);
    }
}
