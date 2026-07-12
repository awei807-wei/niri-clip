// SPDX-License-Identifier: GPL-3.0-only

mod presentation;
mod signals;
mod view;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use gtk4_layer_shell::{KeyboardMode, LayerShell};

use crate::search;
use crate::storage::HistoryItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiAction {
    Refresh,
    Restore(i64),
    Delete(i64),
    Clear,
    SetPaused(bool),
}

pub struct OverlayUi {
    window: gtk::ApplicationWindow,
    root: gtk::Box,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    stack: gtk::Stack,
    state_title: gtk::Label,
    state_detail: gtk::Label,
    pause_button: gtk::ToggleButton,
    clear_button: gtk::Button,
    delete_button: gtk::Button,
    item_counter: gtk::Label,
    mode_label: gtk::Label,
    status_revealer: gtk::Revealer,
    status_label: gtk::Label,
    items: RefCell<Vec<HistoryItem>>,
    visible_ids: RefCell<Vec<i64>>,
    actions: Sender<UiAction>,
    updating_pause: Cell<bool>,
    clear_armed: Cell<bool>,
}

impl OverlayUi {
    pub fn new(application: &gtk::Application, actions: Sender<UiAction>) -> Rc<Self> {
        let widgets = view::build(application);
        let backdrop = widgets.backdrop.clone();
        let delete_button = widgets.delete_button.clone();
        let close_button = widgets.close_button.clone();
        let ui = Rc::new(Self {
            window: widgets.window,
            root: widgets.root,
            search: widgets.search,
            list: widgets.list,
            scroller: widgets.scroller,
            stack: widgets.stack,
            state_title: widgets.state_title,
            state_detail: widgets.state_detail,
            pause_button: widgets.pause_button,
            clear_button: widgets.clear_button,
            delete_button: widgets.delete_button,
            item_counter: widgets.item_counter,
            mode_label: widgets.mode_label,
            status_revealer: widgets.status_revealer,
            status_label: widgets.status_label,
            items: RefCell::new(Vec::new()),
            visible_ids: RefCell::new(Vec::new()),
            actions,
            updating_pause: Cell::new(false),
            clear_armed: Cell::new(false),
        });
        signals::connect(&ui, delete_button, close_button, backdrop);
        ui
    }

    pub fn show(&self) {
        presentation::select_focused_monitor(&self.window);
        self.show_state(
            "LOADING HISTORY",
            "SQLite 历史正在准备，输入焦点已就绪。",
            false,
        );
        self.window.set_keyboard_mode(KeyboardMode::Exclusive);
        self.window.present();
        let search = self.search.clone();
        glib::idle_add_local_once(move || {
            let _ = search.grab_focus();
        });
        let _ = self.actions.send(UiAction::Refresh);
    }

    pub fn hide(&self) {
        self.window.set_keyboard_mode(KeyboardMode::None);
        self.window.set_visible(false);
        self.search.set_text("");
        self.disarm_clear();
    }

    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }

    pub fn set_items(&self, items: Vec<HistoryItem>) {
        self.clear_button.set_sensitive(!items.is_empty());
        self.items.replace(items);
        self.apply_filter();
    }

    pub fn set_paused(&self, paused: bool) {
        self.updating_pause.set(true);
        self.pause_button.set_active(paused);
        self.pause_button
            .set_label(if paused { "RESUME" } else { "PAUSE" });
        self.mode_label
            .set_text(if paused { "PAUSED" } else { "REC" });
        if paused {
            self.root.add_css_class("paused");
            self.flash("记录已暂停；当前剪贴板不会进入历史。", true);
        } else {
            self.root.remove_css_class("paused");
            self.flash("记录已恢复；只捕获恢复之后的新变化。", false);
        }
        self.updating_pause.set(false);
    }

    pub fn show_error(&self, message: &str) {
        self.show_state("HISTORY ERROR", message, true);
    }

    pub fn flash(&self, message: &str, warning: bool) {
        self.status_label.set_text(message);
        if warning {
            self.status_label.add_css_class("warning");
        } else {
            self.status_label.remove_css_class("warning");
        }
        self.status_revealer.set_reveal_child(true);
        let revealer = self.status_revealer.clone();
        glib::timeout_add_local_once(Duration::from_millis(1600), move || {
            revealer.set_reveal_child(false);
        });
    }

    fn apply_filter(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let query = self.search.text();
        let items = self.items.borrow();
        let matches = search::rank(&items, query.as_str());
        self.visible_ids.replace(matched_ids(&items, &matches));

        if items.is_empty() {
            self.show_empty_history();
        } else if matches.is_empty() {
            self.show_state(
                "NO MATCHES",
                &format!("“{}”未匹配任何历史；继续输入或删除字符。", query),
                false,
            );
        } else {
            self.show_matches(&items, &matches);
        }
        self.delete_button.set_sensitive(!matches.is_empty());
        self.update_counter();
    }

    fn show_empty_history(&self) {
        self.show_state(
            "EMPTY HISTORY",
            "复制文字、图片或文件后，它会出现在这里。暂停记录时不会保存任何新内容。",
            false,
        );
    }

    fn show_matches(&self, items: &[HistoryItem], matches: &[usize]) {
        for index in matches {
            self.list.append(&presentation::history_row(&items[*index]));
        }
        self.stack.set_visible_child_name("results");
        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        }
    }

    fn show_state(&self, title: &str, detail: &str, error: bool) {
        self.state_title.set_text(title);
        self.state_detail.set_text(detail);
        if error {
            self.state_title.add_css_class("error");
        } else {
            self.state_title.remove_css_class("error");
        }
        self.stack.set_visible_child_name("state");
    }

    fn update_counter(&self) {
        let total = self.visible_ids.borrow().len();
        let current = self
            .list
            .selected_row()
            .map_or(0, |row| row.index() as usize + 1);
        self.item_counter
            .set_text(&format!("{current:02}/{total:02}"));
    }

    fn move_selection(&self, delta: i32) {
        let count = self.visible_ids.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let current = self.list.selected_row().map_or(0, |row| row.index());
        let next = (current + delta).clamp(0, count - 1);
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
            self.scroll_to_row(&row);
            let _ = self.search.grab_focus();
            self.update_counter();
        }
    }

    fn scroll_to_row(&self, row: &gtk::ListBoxRow) {
        let adjustment = self.scroller.vadjustment();
        let allocation = row.allocation();
        let top = f64::from(allocation.y());
        let bottom = top + f64::from(allocation.height());
        let viewport_top = adjustment.value();
        let viewport_bottom = viewport_top + adjustment.page_size();
        if top < viewport_top {
            adjustment.set_value(top);
        } else if bottom > viewport_bottom {
            adjustment.set_value(bottom - adjustment.page_size());
        }
    }

    fn restore_selected(&self) {
        if let Some(row) = self.list.selected_row() {
            self.restore_at(row.index());
        }
    }

    fn restore_at(&self, index: i32) {
        let Some(id) = self.visible_ids.borrow().get(index as usize).copied() else {
            return;
        };
        let _ = self.actions.send(UiAction::Restore(id));
    }

    fn delete_selected(&self) {
        let Some(row) = self.list.selected_row() else {
            self.flash("当前没有可删除的历史。", true);
            return;
        };
        let Some(id) = self.visible_ids.borrow().get(row.index() as usize).copied() else {
            return;
        };
        let _ = self.actions.send(UiAction::Delete(id));
    }

    fn request_clear(self: &Rc<Self>) {
        if self.items.borrow().is_empty() {
            self.flash("历史已经是空的。", false);
            return;
        }
        if self.clear_armed.replace(true) {
            self.disarm_clear();
            let _ = self.actions.send(UiAction::Clear);
            return;
        }
        self.clear_button.set_label("CONFIRM CLEAR");
        self.clear_button.add_css_class("danger-button");
        self.flash("清空不可撤销；三秒内再次点击确认。", true);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(3), move || {
            if let Some(ui) = weak.upgrade() {
                ui.disarm_clear();
            }
        });
    }

    fn disarm_clear(&self) {
        self.clear_armed.set(false);
        self.clear_button.set_label("CLEAR");
        self.clear_button.remove_css_class("danger-button");
    }
}

fn matched_ids(items: &[HistoryItem], indices: &[usize]) -> Vec<i64> {
    indices.iter().map(|index| items[*index].id).collect()
}

#[cfg(test)]
mod tests {
    use crate::content::ContentKind;
    use crate::storage::HistoryItem;

    use super::matched_ids;

    fn item(id: i64, text: &str) -> HistoryItem {
        HistoryItem {
            id,
            kind: ContentKind::Text,
            mime_type: "text/plain".to_owned(),
            title: text.to_owned(),
            search_text: text.to_owned(),
            byte_len: text.len(),
            thumbnail: None,
            created_at_ms: id,
        }
    }

    #[test]
    fn filtered_rows_keep_their_stable_history_ids() {
        let items = vec![item(41, "alpha"), item(17, "beta"), item(99, "alphabet")];

        assert_eq!(
            matched_ids(&items, &crate::search::rank(&items, "beta")),
            vec![17]
        );
        assert_eq!(
            matched_ids(&items, &crate::search::rank(&items, "alph")),
            vec![41, 99]
        );
    }
}
