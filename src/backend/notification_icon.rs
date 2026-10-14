use gtk::{gdk_pixbuf, glib, prelude::PixbufLoaderExt};

/// Round only the notification copy, preserving the cached avatar and its
/// colours. A radius of one fifth of the shorter side keeps straight edges.
pub(super) fn rounded(image: &[u8]) -> Option<Vec<u8>> {
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader.write(image).ok()?;
    loader.close().ok()?;
    let source = loader.pixbuf()?;
    let (width, height) = (source.width(), source.height());
    let radius = f64::from(width.min(height)) * 0.2;
    let original = source.read_pixel_bytes();
    let channels = source.n_channels() as usize;
    let stride = source.rowstride() as usize;
    let has_alpha = source.has_alpha();
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let at = y as usize * stride + x as usize * channels;
            let pixel = &original[at..at + channels];
            let dx = (radius - (f64::from(x) + 0.5).min(f64::from(width - x) - 0.5)).max(0.0);
            let dy = (radius - (f64::from(y) + 0.5).min(f64::from(height - y) - 0.5)).max(0.0);
            // A one-pixel transition antialiases the curved alpha mask.
            let coverage = if dx == 0.0 || dy == 0.0 {
                1.0
            } else {
                (radius + 0.5 - dx.hypot(dy)).clamp(0.0, 1.0)
            };
            let alpha = if has_alpha { pixel[3] } else { 255 };
            pixels.extend_from_slice(&pixel[..3]);
            pixels.push((f64::from(alpha) * coverage).round() as u8);
        }
    }
    gdk_pixbuf::Pixbuf::from_bytes(
        &glib::Bytes::from_owned(pixels),
        gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        width,
        height,
        width * 4,
    )
    .save_to_bufferv("png", &[])
    .ok()
}
