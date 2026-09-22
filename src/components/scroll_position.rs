use adw::prelude::*;

pub fn preserve(
    scroll: &gtk::ScrolledWindow,
    container: &impl IsA<gtk::Widget>,
    change: impl FnOnce(),
) {
    preserve_inner(scroll, container.first_child(), change);
}

/// Lists can reorder their rows. Preserve the viewport's offset instead of
/// following the old top row to its new position in the list.
pub fn preserve_offset(scroll: &gtk::ScrolledWindow, change: impl FnOnce()) {
    preserve_inner(scroll, None, change);
}

fn preserve_inner(
    scroll: &gtk::ScrolledWindow,
    mut child: Option<gtk::Widget>,
    change: impl FnOnce(),
) {
    let value = scroll.vadjustment().value();
    let mut anchor = None;
    while let Some(widget) = child {
        if let Some(bounds) = widget.compute_bounds(scroll)
            && bounds.y() + bounds.height() > 0.0
        {
            anchor = Some((widget.downgrade(), bounds.y()));
            break;
        }
        child = widget.next_sibling();
    }
    change();
    let weak = scroll.downgrade();
    let laid_out = std::cell::Cell::new(false);
    scroll.add_tick_callback(move |_, _| {
        if !laid_out.replace(true) {
            return gtk::glib::ControlFlow::Continue;
        }
        if let Some(scroll) = weak.upgrade() {
            let adjustment = scroll.vadjustment();
            if (adjustment.value() - value).abs() < 1.0 {
                let delta = anchor
                    .as_ref()
                    .and_then(|(weak, y)| {
                        weak.upgrade()
                            .and_then(|widget| widget.compute_bounds(&scroll))
                            .map(|bounds| f64::from(bounds.y() - y))
                    })
                    .unwrap_or(0.0);
                adjustment.set_value((value + delta).clamp(
                    adjustment.lower(),
                    (adjustment.upper() - adjustment.page_size()).max(adjustment.lower()),
                ));
            }
        }
        gtk::glib::ControlFlow::Break
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires a graphical session and a mapped window"]
    fn reordering_rows_does_not_scroll_after_layout() {
        gtk::init().unwrap();
        let list = gtk::ListBox::new();
        for index in 0..30 {
            let label = gtk::Label::new(Some(&format!("Sender {index}")));
            label.set_size_request(-1, 40);
            list.append(&label);
        }
        let scroll = gtk::ScrolledWindow::builder().child(&list).build();
        let window = gtk::Window::builder()
            .default_width(300)
            .default_height(200)
            .child(&scroll)
            .build();
        fn layout() {
            let context = gtk::glib::MainContext::default();
            let until = std::time::Instant::now() + std::time::Duration::from_millis(150);
            while std::time::Instant::now() < until {
                while context.pending() {
                    context.iteration(false);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        window.present();
        layout();
        for offset in [0.0, 200.0] {
            scroll.vadjustment().set_value(offset);
            layout();
            let row = (0..30)
                .filter_map(|index| list.row_at_index(index))
                .find(|row| {
                    row.compute_bounds(&scroll)
                        .is_some_and(|bounds| bounds.y() + bounds.height() > 0.0)
                })
                .unwrap();
            preserve_offset(&scroll, || {
                list.remove(&row);
                list.append(&row);
            });
            layout();
            assert!((scroll.vadjustment().value() - offset).abs() < 1.0);
        }
        window.close();
    }
}
