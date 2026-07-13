// SPDX-License-Identifier: GPL-3.0-only

use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use super::presentation;

pub(super) struct View {
    pub window: gtk::ApplicationWindow,
    pub backdrop: gtk::Box,
    pub root: gtk::Box,
    pub search: gtk::SearchEntry,
    pub list: gtk::ListBox,
    pub scroller: gtk::ScrolledWindow,
    pub stack: gtk::Stack,
    pub state_title: gtk::Label,
    pub state_detail: gtk::Label,
    pub pause_button: gtk::ToggleButton,
    pub clear_button: gtk::Button,
    pub close_button: gtk::Button,
    pub delete_button: gtk::Button,
    pub item_counter: gtk::Label,
    pub mode_label: gtk::Label,
    pub status_revealer: gtk::Revealer,
    pub status_label: gtk::Label,
    pub preview_revealer: gtk::Revealer,
    pub preview_stack: gtk::Stack,
    pub preview_picture: gtk::Picture,
    pub preview_state_title: gtk::Label,
    pub preview_state_detail: gtk::Label,
}

struct ContentView {
    container: gtk::Overlay,
    stack: gtk::Stack,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    state_title: gtk::Label,
    state_detail: gtk::Label,
    preview_revealer: gtk::Revealer,
    preview_stack: gtk::Stack,
    preview_picture: gtk::Picture,
    preview_state_title: gtk::Label,
    preview_state_detail: gtk::Label,
}

struct PreviewView {
    revealer: gtk::Revealer,
    stack: gtk::Stack,
    picture: gtk::Picture,
    state_title: gtk::Label,
    state_detail: gtk::Label,
}

pub(super) fn build(application: &gtk::Application) -> View {
    presentation::install_css();
    let (window, backdrop, root) = build_shell(application);
    let (header, item_counter, mode_label, close_button) = build_header();
    root.append(&header);
    let search = build_search();
    root.append(&search);
    let (status_revealer, status_label) = build_status();
    root.append(&status_revealer);
    let ContentView {
        container,
        stack,
        list,
        scroller,
        state_title,
        state_detail,
        preview_revealer,
        preview_stack,
        preview_picture,
        preview_state_title,
        preview_state_detail,
    } = build_content();
    root.append(&container);
    let (footer, pause_button, delete_button, clear_button) = build_footer();
    root.append(&footer);

    View {
        window,
        backdrop,
        root,
        search,
        list,
        scroller,
        stack,
        state_title,
        state_detail,
        pause_button,
        clear_button,
        close_button,
        delete_button,
        item_counter,
        mode_label,
        status_revealer,
        status_label,
        preview_revealer,
        preview_stack,
        preview_picture,
        preview_state_title,
        preview_state_detail,
    }
}

fn build_shell(application: &gtk::Application) -> (gtk::ApplicationWindow, gtk::Box, gtk::Box) {
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("niri-clip")
        .decorated(false)
        .build();
    window.init_layer_shell();
    window.set_namespace(Some("niri-clip"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(0);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }

    let host = gtk::Overlay::new();
    host.add_css_class("overlay-host");
    let backdrop = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    backdrop.set_hexpand(true);
    backdrop.set_vexpand(true);
    backdrop.add_css_class("overlay-backdrop");
    host.set_child(Some(&backdrop));

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_size_request(660, 470);
    root.set_halign(gtk::Align::Center);
    root.set_valign(gtk::Align::Center);
    root.add_css_class("overlay-shell");
    host.add_overlay(&root);
    window.set_child(Some(&host));
    (window, backdrop, root)
}

fn build_header() -> (gtk::Box, gtk::Label, gtk::Label, gtk::Button) {
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("header");
    let title = gtk::Label::new(Some("[ CLIP_HISTORY ]"));
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.add_css_class("product-title");
    let item_counter = gtk::Label::new(Some("00/00"));
    item_counter.add_css_class("item-counter");
    let mode = gtk::Label::new(Some("REC"));
    mode.add_css_class("mode-status");
    let close = quiet_button("CLOSE ×", "关闭剪贴板历史（Esc）");
    close.add_css_class("close-button");
    header.append(&title);
    header.append(&item_counter);
    header.append(&mode);
    header.append(&close);
    (header, item_counter, mode, close)
}

fn build_search() -> gtk::SearchEntry {
    let search = gtk::SearchEntry::builder()
        .placeholder_text("SEARCH CLIPBOARD…")
        .hexpand(true)
        .build();
    search.add_css_class("history-search");
    search.set_accessible_role(gtk::AccessibleRole::SearchBox);
    search
}

fn build_status() -> (gtk::Revealer, gtk::Label) {
    let label = gtk::Label::new(None);
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.add_css_class("status-banner");
    let revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(160)
        .child(&label)
        .build();
    (revealer, label)
}

fn build_content() -> ContentView {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Single);
    list.set_activate_on_single_click(false);
    list.set_focusable(false);
    list.add_css_class("history-list");
    let scroller = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&list)
        .build();
    scroller.add_css_class("results-surface");

    let state_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    state_box.set_halign(gtk::Align::Center);
    state_box.set_valign(gtk::Align::Center);
    state_box.set_vexpand(true);
    state_box.add_css_class("state-box");
    let title = gtk::Label::new(None);
    title.add_css_class("state-title");
    let detail = gtk::Label::new(None);
    detail.set_wrap(true);
    detail.set_justify(gtk::Justification::Center);
    detail.set_max_width_chars(46);
    detail.add_css_class("state-detail");
    state_box.append(&title);
    state_box.append(&detail);

    let stack = gtk::Stack::builder()
        .vexpand(true)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(140)
        .build();
    stack.add_named(&scroller, Some("results"));
    stack.add_named(&state_box, Some("state"));

    let PreviewView {
        revealer: preview_revealer,
        stack: preview_stack,
        picture: preview_picture,
        state_title: preview_state_title,
        state_detail: preview_state_detail,
    } = build_preview();
    let container = gtk::Overlay::new();
    container.set_vexpand(true);
    container.set_child(Some(&stack));
    container.add_overlay(&preview_revealer);
    container.set_clip_overlay(&preview_revealer, true);

    ContentView {
        container,
        stack,
        list,
        scroller,
        state_title: title,
        state_detail: detail,
        preview_revealer,
        preview_stack,
        preview_picture,
        preview_state_title,
        preview_state_detail,
    }
}

fn build_preview() -> PreviewView {
    let picture = gtk::Picture::builder()
        .can_shrink(true)
        .content_fit(gtk::ContentFit::Contain)
        .build();
    picture.add_css_class("image-preview-picture");

    let loading = preview_state("LOADING PREVIEW", "正在读取完整图片…", false);
    let error_title = gtk::Label::new(Some("PREVIEW UNAVAILABLE"));
    error_title.add_css_class("image-preview-state-title");
    let error_detail = gtk::Label::new(Some("无法读取或解码图片；仍可按 Enter 恢复。"));
    error_detail.set_wrap(true);
    error_detail.set_justify(gtk::Justification::Center);
    error_detail.set_max_width_chars(32);
    error_detail.add_css_class("image-preview-state-detail");
    let error = gtk::Box::new(gtk::Orientation::Vertical, 8);
    error.set_halign(gtk::Align::Center);
    error.set_valign(gtk::Align::Center);
    error.add_css_class("image-preview-state");
    error.add_css_class("error");
    error.append(&error_title);
    error.append(&error_detail);

    let stack = gtk::Stack::builder()
        .hexpand(true)
        .vexpand(true)
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(140)
        .build();
    stack.add_named(&loading, Some("loading"));
    stack.add_named(&picture, Some("image"));
    stack.add_named(&error, Some("error"));
    stack.set_visible_child_name("loading");

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.set_size_request(320, 240);
    panel.set_can_target(false);
    panel.set_focusable(false);
    panel.add_css_class("image-preview-panel");
    panel.append(&stack);

    let revealer = gtk::Revealer::builder()
        .halign(gtk::Align::End)
        .valign(gtk::Align::Start)
        .margin_top(11)
        .margin_end(25)
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(140)
        .child(&panel)
        .build();
    revealer.set_can_target(false);
    revealer.set_focusable(false);
    revealer.add_css_class("image-preview-revealer");

    PreviewView {
        revealer,
        stack,
        picture,
        state_title: error_title,
        state_detail: error_detail,
    }
}

fn preview_state(title: &str, detail: &str, error: bool) -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 8);
    container.set_halign(gtk::Align::Center);
    container.set_valign(gtk::Align::Center);
    container.add_css_class("image-preview-state");
    if error {
        container.add_css_class("error");
    }
    let title = gtk::Label::new(Some(title));
    title.add_css_class("image-preview-state-title");
    let detail = gtk::Label::new(Some(detail));
    detail.set_wrap(true);
    detail.set_justify(gtk::Justification::Center);
    detail.set_max_width_chars(32);
    detail.add_css_class("image-preview-state-detail");
    container.append(&title);
    container.append(&detail);
    container
}

fn build_footer() -> (gtk::Box, gtk::ToggleButton, gtk::Button, gtk::Button) {
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("footer");
    let help = gtk::Label::new(Some("↑↓ SELECT  ·  ENTER PASTE  ·  ESC CLOSE"));
    help.set_xalign(0.0);
    help.set_hexpand(true);
    help.add_css_class("key-help");
    let pause = quiet_toggle("PAUSE", "暂停后不会读取或保存新的剪贴板内容");
    let delete = quiet_button("REMOVE", "删除当前选中的历史（Shift+Delete）");
    let clear = quiet_button("CLEAR", "清空全部剪贴板历史（Ctrl+Shift+Delete）");
    clear.add_css_class("clear-button");
    footer.append(&help);
    footer.append(&pause);
    footer.append(&delete);
    footer.append(&clear);
    (footer, pause, delete, clear)
}

fn quiet_button(label: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("quiet-button");
    button
}

fn quiet_toggle(label: &str, tooltip: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::with_label(label);
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("quiet-button");
    button
}
