// SPDX-License-Identifier: GPL-3.0-only

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::{Context, Result};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use crate::clipboard::{self, ClipboardEvent};
use crate::config::{AppConfig, AppPaths};
use crate::ipc::{self, Envelope, Request, Response};
use crate::storage::{RecordOutcome, Storage};
use crate::ui::{OverlayUi, UiAction};

pub fn run(paths: AppPaths, config: AppConfig) -> Result<()> {
    let (listener, _socket_guard) = ipc::bind(&paths.socket)?;
    let storage = Storage::open(&paths.database, config.max_items)?;
    let initial_paused = storage.is_paused()?;
    let recording_gate = clipboard::RecordingGate::new(initial_paused);

    let (ipc_tx, ipc_rx) = mpsc::channel();
    let _ipc_thread = ipc::start_server(listener, ipc_tx);
    let (clipboard_tx, clipboard_rx) = mpsc::channel();
    clipboard::start_monitor(clipboard_tx, recording_gate.clone(), config.max_bytes)?;
    let (ui_tx, ui_rx) = mpsc::channel();

    let application = gtk::Application::builder()
        .application_id("io.github.niri_clip.NiriClip")
        .flags(gtk::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let storage = Rc::new(RefCell::new(storage));
    let ui_slot = Rc::new(RefCell::new(None::<Rc<OverlayUi>>));
    let receivers = Rc::new(RefCell::new(Some((ipc_rx, clipboard_rx, ui_rx))));

    let activation_storage = storage.clone();
    let activation_gate = recording_gate.clone();
    let activation_slot = ui_slot.clone();
    let activation_receivers = receivers.clone();
    application.connect_activate(move |application| {
        if activation_slot.borrow().is_some() {
            return;
        }
        let Some((ipc_rx, clipboard_rx, ui_rx)) = activation_receivers.borrow_mut().take() else {
            return;
        };

        let ui = OverlayUi::new(application, ui_tx.clone());
        ui.set_paused(activation_gate.is_paused());
        activation_slot.replace(Some(ui.clone()));

        let storage = activation_storage.clone();
        let recording_gate = activation_gate.clone();
        let ui = ui.clone();
        glib::timeout_add_local(Duration::from_millis(20), move || {
            drain_clipboard(&clipboard_rx, &storage, &ui, &recording_gate);
            drain_ui(&ui_rx, &storage, &ui, &recording_gate);
            drain_ipc(&ipc_rx, &storage, &ui, &recording_gate);
            glib::ControlFlow::Continue
        });
    });

    let _hold = application.hold();
    application.run_with_args(&["niri-clip"]);
    Ok(())
}

fn drain_clipboard(
    receiver: &Receiver<ClipboardEvent>,
    storage: &Rc<RefCell<Storage>>,
    ui: &Rc<OverlayUi>,
    recording_gate: &clipboard::RecordingGate,
) {
    while let Ok(event) = receiver.try_recv() {
        match event {
            ClipboardEvent::Captured { content, epoch } => {
                if !recording_gate.accepts(epoch) {
                    continue;
                }
                let thumbnail = crate::media::thumbnail_png(&content);
                match storage.borrow_mut().record(&content, thumbnail.as_deref()) {
                    Ok(RecordOutcome::Inserted(_)) if ui.is_visible() => refresh(storage, ui),
                    Ok(_) => {}
                    Err(error) => {
                        let message = format!("保存剪贴板历史失败: {error:#}");
                        eprintln!("{message}");
                        if ui.is_visible() {
                            ui.show_error(&message);
                        }
                    }
                }
            }
            ClipboardEvent::BackendReady(backend) => {
                eprintln!("剪贴板监听后端: {backend}");
            }
            ClipboardEvent::Warning(message) => {
                eprintln!("{message}");
                if ui.is_visible() {
                    ui.flash(&message, true);
                }
            }
        }
    }
}

fn drain_ui(
    receiver: &Receiver<UiAction>,
    storage: &Rc<RefCell<Storage>>,
    ui: &Rc<OverlayUi>,
    recording_gate: &clipboard::RecordingGate,
) {
    while let Ok(action) = receiver.try_recv() {
        match action {
            UiAction::Refresh => refresh(storage, ui),
            UiAction::Preview(id) if ui.is_visible() => {
                if !ui.begin_preview(id) {
                    continue;
                }
                let storage = storage.clone();
                let ui = Rc::downgrade(ui);
                glib::idle_add_local_once(move || {
                    let Some(ui) = ui.upgrade() else {
                        return;
                    };
                    if !ui.should_load_preview(id) {
                        return;
                    }
                    let result = load_preview(&storage, id);
                    match result {
                        Ok(preview) => ui.finish_preview(id, preview),
                        Err(error) => {
                            eprintln!("读取图片预览失败: {error:#}");
                            ui.fail_preview(id);
                        }
                    }
                });
            }
            UiAction::Preview(_) => {}
            UiAction::HidePreview(id) => ui.hide_preview(id),
            UiAction::Restore(id) if ui.is_visible() => match restore(storage, id) {
                Ok(()) => {
                    ui.hide();
                    let ui = Rc::downgrade(ui);
                    crate::auto_paste::schedule(move || {
                        ui.upgrade().is_some_and(|ui| !ui.is_visible())
                    });
                }
                Err(error) => ui.flash(&format!("恢复失败: {error:#}"), true),
            },
            UiAction::Restore(_) => {}
            UiAction::Delete(id) => match delete(storage, id) {
                Ok(true) => {
                    refresh(storage, ui);
                    ui.flash("已删除一条历史。", false);
                }
                Ok(false) => ui.flash("该历史已不存在。", true),
                Err(error) => ui.flash(&format!("删除失败: {error:#}"), true),
            },
            UiAction::Clear => match clear(storage) {
                Ok(count) => {
                    refresh(storage, ui);
                    ui.flash(&format!("已清空 {count} 条历史。"), false);
                }
                Err(error) => ui.flash(&format!("清空失败: {error:#}"), true),
            },
            UiAction::SetPaused(value) => {
                if let Err(error) = set_paused(storage, ui, recording_gate, value) {
                    ui.flash(&format!("更新暂停状态失败: {error:#}"), true);
                }
            }
        }
    }
}

fn drain_ipc(
    receiver: &Receiver<Envelope>,
    storage: &Rc<RefCell<Storage>>,
    ui: &Rc<OverlayUi>,
    recording_gate: &clipboard::RecordingGate,
) {
    while let Ok(envelope) = receiver.try_recv() {
        let response = handle_ipc_request(envelope.request, storage, ui, recording_gate);
        let _ = envelope.response.send(response);
    }
}

fn handle_ipc_request(
    request: Request,
    storage: &Rc<RefCell<Storage>>,
    ui: &Rc<OverlayUi>,
    recording_gate: &clipboard::RecordingGate,
) -> Response {
    match request {
        Request::Show => show_response(ui),
        Request::Toggle => toggle_response(ui),
        Request::Hide => hide_response(ui),
        Request::Pause => pause_response(storage, ui, recording_gate, true),
        Request::Resume => pause_response(storage, ui, recording_gate, false),
        Request::TogglePause => {
            pause_response(storage, ui, recording_gate, !recording_gate.is_paused())
        }
        Request::Delete { id } => delete_response(storage, ui, id),
        Request::Clear => clear_response(storage, ui),
        Request::Status => status_response(storage, recording_gate),
        Request::Ping => Response::ok("pong"),
    }
}

fn show_response(ui: &OverlayUi) -> Response {
    ui.show();
    Response::ok("Overlay 已显示")
}

fn toggle_response(ui: &OverlayUi) -> Response {
    if ui.is_visible() {
        hide_response(ui)
    } else {
        show_response(ui)
    }
}

fn hide_response(ui: &OverlayUi) -> Response {
    ui.hide();
    Response::ok("Overlay 已关闭")
}

fn delete_response(storage: &Rc<RefCell<Storage>>, ui: &OverlayUi, id: i64) -> Response {
    match delete(storage, id) {
        Ok(true) => {
            refresh_if_visible(storage, ui);
            Response::ok(format!("已删除历史 {id}"))
        }
        Ok(false) => Response::error(format!("历史 {id} 不存在")),
        Err(error) => Response::error(format!("删除失败: {error:#}")),
    }
}

fn clear_response(storage: &Rc<RefCell<Storage>>, ui: &OverlayUi) -> Response {
    match clear(storage) {
        Ok(count) => {
            refresh_if_visible(storage, ui);
            Response::ok(format!("已清空 {count} 条历史"))
        }
        Err(error) => Response::error(format!("清空失败: {error:#}")),
    }
}

fn status_response(
    storage: &Rc<RefCell<Storage>>,
    recording_gate: &clipboard::RecordingGate,
) -> Response {
    match storage.borrow().count() {
        Ok(count) => Response {
            ok: true,
            message: "ready".to_owned(),
            paused: Some(recording_gate.is_paused()),
            count: Some(count),
        },
        Err(error) => Response::error(format!("读取状态失败: {error:#}")),
    }
}

fn refresh_if_visible(storage: &Rc<RefCell<Storage>>, ui: &OverlayUi) {
    if ui.is_visible() {
        refresh(storage, ui);
    }
}

fn refresh(storage: &Rc<RefCell<Storage>>, ui: &OverlayUi) {
    match storage.borrow().list() {
        Ok(items) => ui.set_items(items),
        Err(error) => ui.show_error(&format!("读取 SQLite 历史失败: {error:#}")),
    }
}

fn restore(storage: &Rc<RefCell<Storage>>, id: i64) -> Result<()> {
    let content = storage
        .borrow()
        .get_content(id)?
        .with_context(|| format!("历史 {id} 不存在"))?;
    clipboard::copy_content(&content)
}

fn load_preview(storage: &Rc<RefCell<Storage>>, id: i64) -> Result<Option<Vec<u8>>> {
    Ok(storage
        .borrow()
        .get_content(id)?
        .as_ref()
        .and_then(crate::media::preview_png))
}

fn delete(storage: &Rc<RefCell<Storage>>, id: i64) -> Result<bool> {
    storage.borrow().delete(id)
}

fn clear(storage: &Rc<RefCell<Storage>>) -> Result<usize> {
    storage.borrow().clear()
}

fn set_paused(
    storage: &Rc<RefCell<Storage>>,
    ui: &OverlayUi,
    recording_gate: &clipboard::RecordingGate,
    value: bool,
) -> Result<()> {
    storage.borrow().set_paused(value)?;
    recording_gate.set_paused(value);
    ui.set_paused(value);
    Ok(())
}

fn pause_response(
    storage: &Rc<RefCell<Storage>>,
    ui: &OverlayUi,
    recording_gate: &clipboard::RecordingGate,
    value: bool,
) -> Response {
    match set_paused(storage, ui, recording_gate, value) {
        Ok(()) => Response::ok(if value {
            "记录已暂停"
        } else {
            "记录已恢复"
        }),
        Err(error) => Response::error(format!("更新暂停状态失败: {error:#}")),
    }
}
