// SPDX-License-Identifier: GPL-3.0-only

use std::rc::Rc;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use super::{OverlayUi, UiAction};

pub(super) fn connect(
    ui: &Rc<OverlayUi>,
    delete_button: gtk::Button,
    close_button: gtk::Button,
    backdrop: gtk::Box,
) {
    connect_search_and_rows(ui);
    connect_action_buttons(ui, delete_button);
    connect_keyboard(ui);
    connect_close_paths(ui, close_button, backdrop);
}

fn connect_search_and_rows(ui: &Rc<OverlayUi>) {
    let weak = Rc::downgrade(ui);
    ui.search.connect_search_changed(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.cancel_preview();
            ui.apply_filter();
        }
    });
    let weak = Rc::downgrade(ui);
    ui.search.connect_activate(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.cancel_preview();
            ui.restore_selected();
        }
    });
    let weak = Rc::downgrade(ui);
    ui.list.connect_row_activated(move |_, row| {
        if let Some(ui) = weak.upgrade() {
            ui.cancel_preview();
            ui.restore_at(row.index());
        }
    });
    let weak = Rc::downgrade(ui);
    ui.list.connect_row_selected(move |_, _| {
        if let Some(ui) = weak.upgrade() {
            ui.cancel_preview();
            ui.update_counter();
        }
    });
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
    scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(ui);
    scroll.connect_scroll(move |_, _, _| {
        if let Some(ui) = weak.upgrade() {
            ui.cancel_preview();
        }
        glib::Propagation::Proceed
    });
    ui.scroller.add_controller(scroll);
    let weak = Rc::downgrade(ui);
    ui.scroller.vadjustment().connect_value_changed(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.cancel_preview();
        }
    });
}

fn connect_action_buttons(ui: &Rc<OverlayUi>, delete_button: gtk::Button) {
    let weak = Rc::downgrade(ui);
    ui.pause_button.connect_toggled(move |button| {
        if let Some(ui) = weak.upgrade() {
            if !ui.updating_pause.get() {
                let _ = ui.actions.send(UiAction::SetPaused(button.is_active()));
            }
        }
    });
    let weak = Rc::downgrade(ui);
    ui.clear_button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.request_clear();
        }
    });
    let weak = Rc::downgrade(ui);
    delete_button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.delete_selected();
        }
    });
}

fn connect_keyboard(ui: &Rc<OverlayUi>) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(ui);
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        weak.upgrade().map_or(glib::Propagation::Proceed, |ui| {
            ui.cancel_preview();
            handle_key(&ui, key, modifiers)
        })
    });
    ui.window.add_controller(controller);
}

fn handle_key(
    ui: &Rc<OverlayUi>,
    key: gdk::Key,
    modifiers: gdk::ModifierType,
) -> glib::Propagation {
    match key {
        gdk::Key::Escape => ui.hide(),
        gdk::Key::Up => ui.move_selection(-1),
        gdk::Key::Down => ui.move_selection(1),
        gdk::Key::Return | gdk::Key::KP_Enter => ui.restore_selected(),
        gdk::Key::Delete if modifiers.contains(gdk::ModifierType::SHIFT_MASK) => {
            if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
                ui.request_clear();
            } else {
                ui.delete_selected();
            }
        }
        _ => return glib::Propagation::Proceed,
    }
    glib::Propagation::Stop
}

fn connect_close_paths(ui: &Rc<OverlayUi>, close_button: gtk::Button, backdrop: gtk::Box) {
    let weak = Rc::downgrade(ui);
    close_button.connect_clicked(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.hide();
        }
    });
    let click = gtk::GestureClick::new();
    let weak = Rc::downgrade(ui);
    click.connect_released(move |_, _, _, _| {
        if let Some(ui) = weak.upgrade() {
            ui.hide();
        }
    });
    backdrop.add_controller(click);
    let search = ui.search.clone();
    ui.window.connect_map(move |_| {
        let _ = search.grab_focus();
    });
    let weak = Rc::downgrade(ui);
    ui.window.connect_close_request(move |_| {
        if let Some(ui) = weak.upgrade() {
            ui.hide();
        }
        glib::Propagation::Stop
    });
}
