// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use gtk::gdk;
use gtk::pango;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::LayerShell;
use serde::Deserialize;

use crate::search;
use crate::storage::HistoryItem;

pub(super) fn history_row(item: &HistoryItem) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_selectable(true);
    row.set_activatable(true);
    row.set_focusable(false);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 5);
    content.add_css_class("history-row-content");
    let preview = gtk::Label::new(Some(&search::preview(&item.content, 140)));
    preview.set_xalign(0.0);
    preview.set_ellipsize(pango::EllipsizeMode::End);
    preview.set_single_line_mode(true);
    preview.add_css_class("history-preview");
    let meta = gtk::Label::new(Some(&format!(
        "{}  ·  {}",
        relative_time(item.created_at_ms),
        byte_size(item.content.len())
    )));
    meta.set_xalign(0.0);
    meta.add_css_class("history-meta");
    content.append(&preview);
    content.append(&meta);
    row.set_child(Some(&content));
    row
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
    let seconds = now.saturating_sub(timestamp_ms) / 1000;
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
    use super::{byte_size, is_hex_color};

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
