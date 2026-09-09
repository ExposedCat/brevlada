use adw::prelude::*;

pub fn preserve(
    scroll: &gtk::ScrolledWindow,
    container: &impl IsA<gtk::Widget>,
    change: impl FnOnce(),
) {
    let value = scroll.vadjustment().value();
    let mut child = container.first_child();
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
