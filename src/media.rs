// SPDX-License-Identifier: GPL-3.0-only

//! Bounded media presentation helpers executed on the GTK main thread.

use gtk::gdk_pixbuf::PixbufLoader;
use gtk::prelude::*;
use gtk4 as gtk;

use crate::content::{ClipboardContent, ContentKind};

pub const THUMBNAIL_WIDTH: i32 = 72;
pub const THUMBNAIL_HEIGHT: i32 = 48;

/// Generates a bounded PNG thumbnail without allocating the source dimensions.
pub fn thumbnail_png(content: &ClipboardContent) -> Option<Vec<u8>> {
    if content.kind != ContentKind::Image {
        return None;
    }
    let loader = PixbufLoader::new();
    loader.connect_size_prepared(|loader, width, height| {
        let (width, height) = fit_dimensions(width, height);
        loader.set_size(width, height);
    });
    loader.write(&content.bytes).ok()?;
    loader.close().ok()?;
    loader.pixbuf()?.save_to_bufferv("png", &[]).ok()
}

fn fit_dimensions(width: i32, height: i32) -> (i32, i32) {
    if width <= 0 || height <= 0 {
        return (THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);
    }
    let scale = (f64::from(THUMBNAIL_WIDTH) / f64::from(width))
        .min(f64::from(THUMBNAIL_HEIGHT) / f64::from(height))
        .min(1.0);
    (
        (f64::from(width) * scale).round().max(1.0) as i32,
        (f64::from(height) * scale).round().max(1.0) as i32,
    )
}

#[cfg(test)]
mod tests {
    use gtk::gdk_pixbuf::{Colorspace, Pixbuf, PixbufLoader};
    use gtk::prelude::*;
    use gtk4 as gtk;

    use crate::content::{ClipboardContent, ContentFormat, ContentKind};

    use super::{thumbnail_png, THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH};

    fn content(kind: ContentKind, mime_type: &str, bytes: Vec<u8>) -> ClipboardContent {
        ClipboardContent::new(
            ContentFormat {
                kind,
                mime_type: mime_type.to_owned(),
            },
            bytes,
        )
        .unwrap()
    }

    #[test]
    fn creates_bounded_png_thumbnail_for_large_image() {
        let source = Pixbuf::new(Colorspace::Rgb, true, 8, 400, 200).unwrap();
        source.fill(0x4a7f6aff);
        let png = source.save_to_bufferv("png", &[]).unwrap();

        let thumbnail = thumbnail_png(&content(ContentKind::Image, "image/png", png)).unwrap();
        let loader = PixbufLoader::new();
        loader.write(&thumbnail).unwrap();
        loader.close().unwrap();
        let decoded = loader.pixbuf().unwrap();

        assert!(decoded.width() <= THUMBNAIL_WIDTH);
        assert!(decoded.height() <= THUMBNAIL_HEIGHT);
        assert_eq!((decoded.width(), decoded.height()), (72, 36));
    }

    #[test]
    fn invalid_images_and_non_image_content_have_no_thumbnail() {
        let invalid = content(ContentKind::Image, "image/png", b"not-an-image".to_vec());
        assert!(thumbnail_png(&invalid).is_none());

        let text = content(ContentKind::Text, "text/plain", b"text".to_vec());
        assert!(thumbnail_png(&text).is_none());
    }
}
