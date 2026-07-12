// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::LayerShell;
use serde::Deserialize;

use crate::search;
use crate::storage::HistoryItem;

#[derive(Debug, Eq, PartialEq)]
struct RowPresentation {
    title: String,
    meta: String,
    accessible_label: String,
}

pub(super) fn history_row(item: &HistoryItem) -> gtk::ListBoxRow {
    let presentation = row_presentation(item);
    let row = gtk::ListBoxRow::new();
    row.set_selectable(true);
    row.set_activatable(true);
    row.set_focusable(false);
    row.update_property(&[gtk::accessible::Property::Label(
        &presentation.accessible_label,
    )]);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.add_css_class("history-row-content");
    if item.kind != crate::content::ContentKind::Text {
        content.append(&media_visual(item));
    }
    let copy = gtk::Box::new(gtk::Orientation::Vertical, 5);
    copy.set_hexpand(true);
    copy.add_css_class("history-row-copy");
    let preview = gtk::Label::new(Some(&presentation.title));
    preview.set_xalign(0.0);
    preview.set_ellipsize(pango::EllipsizeMode::End);
    preview.set_single_line_mode(true);
    preview.add_css_class("history-preview");
    let meta = gtk::Label::new(Some(&presentation.meta));
    meta.set_xalign(0.0);
    meta.set_ellipsize(pango::EllipsizeMode::End);
    meta.set_single_line_mode(true);
    meta.add_css_class("history-meta");
    copy.append(&preview);
    copy.append(&meta);
    content.append(&copy);
    row.set_child(Some(&content));
    row
}

fn media_visual(item: &HistoryItem) -> gtk::Widget {
    if let Some(thumbnail) = item.thumbnail.as_deref().and_then(thumbnail_texture) {
        let picture = gtk::Picture::builder()
            .paintable(&thumbnail)
            .can_shrink(true)
            .content_fit(gtk::ContentFit::Cover)
            .build();
        picture.set_size_request(72, 48);
        picture.add_css_class("media-thumb");
        return picture.upcast();
    }
    let placeholder = gtk::Label::new(Some(item.kind.label()));
    placeholder.set_size_request(72, 48);
    placeholder.add_css_class("media-placeholder");
    placeholder.upcast()
}

fn thumbnail_texture(bytes: &[u8]) -> Option<gdk::Texture> {
    let bytes = glib::Bytes::from_owned(bytes.to_vec());
    gdk::Texture::from_bytes(&bytes).ok()
}

fn row_presentation(item: &HistoryItem) -> RowPresentation {
    let kind = item.kind.label();
    let time = relative_time(item.created_at_ms);
    let size = byte_size(item.byte_len);
    let title = if item.kind == crate::content::ContentKind::Text {
        search::preview(&item.title, 140)
    } else if item.title.trim().is_empty() {
        format!("{kind} 内容")
    } else {
        item.title.clone()
    };
    RowPresentation {
        meta: format!("{kind}  ·  {}  ·  {time}  ·  {size}", item.mime_type),
        accessible_label: format!("{kind}，{title}，{}，{size}，{time}", item.mime_type),
        title,
    }
}

pub(super) fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&themed_css());
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn themed_css() -> String {
    let fallback = "#5a9a8a";
    let accent = matugen_accent().unwrap_or_else(|| fallback.to_owned());
    include_str!("../../assets/style.css").replace(
        "@define-color zen_accent #5a9a8a;",
        &format!("@define-color zen_accent {accent};"),
    )
}

fn matugen_accent() -> Option<String> {
    let path = dirs::cache_dir()?.join("matugen/colors.json");
    let contents = fs::read_to_string(path).ok()?;
    if contents.len() > 64 * 1024 {
        return None;
    }
    let document: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let accent = document.get("colors")?.get("primary")?.as_str()?;
    is_hex_color(accent).then(|| accent.to_owned())
}

fn is_hex_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn select_focused_monitor(window: &gtk::ApplicationWindow) {
    let Some(connector) = focused_output_name() else {
        window.set_monitor(None);
        return;
    };
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let monitors = display.monitors();
    for index in 0..monitors.n_items() {
        let Some(object) = monitors.item(index) else {
            continue;
        };
        let Ok(monitor) = object.downcast::<gdk::Monitor>() else {
            continue;
        };
        if monitor.connector().as_deref() == Some(connector.as_str()) {
            window.set_monitor(Some(&monitor));
            return;
        }
    }
    window.set_monitor(None);
}

#[derive(Deserialize)]
struct FocusedOutput {
    name: String,
}

fn focused_output_name() -> Option<String> {
    let output = Command::new("niri")
        .args(["msg", "--json", "focused-output"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice::<FocusedOutput>(&output.stdout)
        .ok()
        .map(|output| output.name)
}

fn relative_time(timestamp_ms: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let seconds = now.saturating_sub(timestamp_ms).max(0) / 1000;
    match seconds {
        0..=59 => "刚刚".to_owned(),
        60..=3599 => format!("{} 分钟前", seconds / 60),
        3600..=86_399 => format!("{} 小时前", seconds / 3600),
        _ => format!("{} 天前", seconds / 86_400),
    }
}

fn byte_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use crate::content::ContentKind;
    use crate::storage::HistoryItem;

    use super::{byte_size, is_hex_color, row_presentation, RowPresentation};

    fn item(kind: ContentKind, title: &str, mime_type: &str) -> HistoryItem {
        HistoryItem {
            id: 7,
            kind,
            mime_type: mime_type.to_owned(),
            title: title.to_owned(),
            search_text: title.to_owned(),
            byte_len: 1536,
            thumbnail: None,
            created_at_ms: i64::MAX,
        }
    }

    #[test]
    fn media_rows_expose_type_mime_size_and_accessible_description() {
        let row = row_presentation(&item(ContentKind::Video, "demo.mp4", "video/mp4"));

        assert_eq!(
            row,
            RowPresentation {
                title: "demo.mp4".to_owned(),
                meta: "VIDEO  ·  video/mp4  ·  刚刚  ·  1.5 KiB".to_owned(),
                accessible_label: "VIDEO，demo.mp4，video/mp4，1.5 KiB，刚刚".to_owned(),
            }
        );
    }

    #[test]
    fn text_rows_keep_flattened_preview() {
        let row = row_presentation(&item(
            ContentKind::Text,
            "  first\n\tsecond  ",
            "text/plain;charset=utf-8",
        ));
        assert_eq!(row.title, "first second");
    }

    #[test]
    fn formats_byte_sizes_for_row_metadata() {
        assert_eq!(byte_size(42), "42 B");
        assert_eq!(byte_size(1536), "1.5 KiB");
    }

    #[test]
    fn validates_matugen_hex_colors() {
        assert!(is_hex_color("#adc6ff"));
        assert!(!is_hex_color("blue"));
        assert!(!is_hex_color("#12345678"));
        assert!(!is_hex_color("#12zz56"));
    }
}
