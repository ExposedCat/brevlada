use crate::theme;
use gtk::prelude::*;

pub(super) fn new(fraction: f64) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(theme::PROCESS_RING_SIZE)
        .content_height(theme::PROCESS_RING_SIZE)
        .valign(gtk::Align::Center)
        .css_classes(["process-ring"])
        .build();
    area.set_draw_func(move |area, context, width, height| {
        let color = area.color();
        let center = (f64::from(width) / 2.0, f64::from(height) / 2.0);
        let radius = (f64::from(width.min(height)) - theme::PROCESS_RING_STROKE) / 2.0;
        context.set_line_width(theme::PROCESS_RING_STROKE);
        context.set_line_cap(gtk::cairo::LineCap::Round);
        context.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            theme::PROCESS_RING_TRACK_ALPHA,
        );
        context.arc(center.0, center.1, radius, 0.0, std::f64::consts::TAU);
        let _ = context.stroke();
        context.set_source_rgba(
            f64::from(color.red()),
            f64::from(color.green()),
            f64::from(color.blue()),
            f64::from(color.alpha()),
        );
        let start = -std::f64::consts::FRAC_PI_2;
        context.arc(
            center.0,
            center.1,
            radius,
            start,
            start + std::f64::consts::TAU * fraction,
        );
        let _ = context.stroke();
    });
    area
}
