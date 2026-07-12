// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::os::fd::AsFd;
use std::sync::mpsc::{self, Sender, SyncSender};

use anyhow::{bail, Context, Result};
use wayland_client::backend::ObjectId;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{delegate_noop, event_created_child, Dispatch, Proxy, QueueHandle};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_device_v1::{
    self, ExtDataControlDeviceV1,
};
use wayland_protocols::ext::data_control::v1::client::ext_data_control_manager_v1::ExtDataControlManagerV1;
use wayland_protocols::ext::data_control::v1::client::ext_data_control_offer_v1::{
    self, ExtDataControlOfferV1,
};
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_device_v1::{
    self, ZwlrDataControlDeviceV1,
};
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_manager_v1::ZwlrDataControlManagerV1;
use wayland_protocols_wlr::data_control::v1::client::zwlr_data_control_offer_v1::{
    self, ZwlrDataControlOfferV1,
};

use super::transfer::select_text_mime;
use super::{CaptureTrigger, ClipboardEvent, RecordingGate};

enum WatchDevice {
    Ext(ExtDataControlDeviceV1),
    Wlr(ZwlrDataControlDeviceV1),
}

struct WatchState {
    capture: SyncSender<CaptureTrigger>,
    events: Sender<ClipboardEvent>,
    gate: RecordingGate,
    offers: HashMap<ObjectId, Vec<String>>,
    finished: bool,
    backpressure_warned: bool,
}

impl WatchState {
    fn finish_offer(&mut self, id: ObjectId) -> Vec<String> {
        self.offers.remove(&id).unwrap_or_default()
    }

    fn queue_reader(&mut self, reader: os_pipe::PipeReader, epoch: u64) {
        match self
            .capture
            .try_send(CaptureTrigger::Native { reader, epoch })
        {
            Ok(()) => self.backpressure_warned = false,
            Err(mpsc::TrySendError::Full(_)) if !self.backpressure_warned => {
                self.backpressure_warned = true;
                let _ = self.events.send(ClipboardEvent::Warning(
                    "剪贴板变化过快，读取队列已满；已丢弃一个变更事件。".to_owned(),
                ));
            }
            Err(mpsc::TrySendError::Full(_) | mpsc::TrySendError::Disconnected(_)) => {}
        }
    }

    fn receive_ext(
        &mut self,
        offer: ExtDataControlOfferV1,
        connection: &wayland_client::Connection,
    ) -> Result<()> {
        let mimes = self.finish_offer(offer.id());
        let epoch = self.gate.snapshot();
        let Some(mime) = select_text_mime(&mimes).filter(|_| self.gate.accepts(epoch)) else {
            offer.destroy();
            return Ok(());
        };
        let transfer: Result<os_pipe::PipeReader> = (|| {
            let (reader, writer) = os_pipe::pipe().context("无法创建剪贴板传输管道")?;
            offer.receive(mime, writer.as_fd());
            drop(writer);
            connection.flush().context("无法发送剪贴板接收请求")?;
            Ok(reader)
        })();
        offer.destroy();
        self.queue_reader(transfer?, epoch);
        Ok(())
    }

    fn receive_wlr(
        &mut self,
        offer: ZwlrDataControlOfferV1,
        connection: &wayland_client::Connection,
    ) -> Result<()> {
        let mimes = self.finish_offer(offer.id());
        let epoch = self.gate.snapshot();
        let Some(mime) = select_text_mime(&mimes).filter(|_| self.gate.accepts(epoch)) else {
            offer.destroy();
            return Ok(());
        };
        let transfer: Result<os_pipe::PipeReader> = (|| {
            let (reader, writer) = os_pipe::pipe().context("无法创建剪贴板传输管道")?;
            offer.receive(mime, writer.as_fd());
            drop(writer);
            connection.flush().context("无法发送剪贴板接收请求")?;
            Ok(reader)
        })();
        offer.destroy();
        self.queue_reader(transfer?, epoch);
        Ok(())
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for WatchState {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as Proxy>::Event,
        _data: &GlobalListContents,
        _connection: &wayland_client::Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(WatchState: ignore WlSeat);
delegate_noop!(WatchState: ignore ExtDataControlManagerV1);
delegate_noop!(WatchState: ignore ZwlrDataControlManagerV1);

impl Dispatch<ExtDataControlOfferV1, ()> for WatchState {
    fn event(
        state: &mut Self,
        proxy: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        _data: &(),
        _connection: &wayland_client::Connection,
        _queue: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event {
            state.offers.entry(proxy.id()).or_default().push(mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for WatchState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _data: &(),
        _connection: &wayland_client::Connection,
        _queue: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            state.offers.entry(proxy.id()).or_default().push(mime_type);
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for WatchState {
    fn event(
        state: &mut Self,
        _proxy: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _data: &(),
        connection: &wayland_client::Connection,
        _queue: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_device_v1::Event::DataOffer { id } => {
                state.offers.insert(id.id(), Vec::new());
            }
            ext_data_control_device_v1::Event::Selection { id: Some(offer) } => {
                if let Err(error) = state.receive_ext(offer, connection) {
                    let _ = state.events.send(ClipboardEvent::Warning(format!(
                        "接收 ext-data-control 剪贴板失败: {error:#}"
                    )));
                }
            }
            ext_data_control_device_v1::Event::PrimarySelection { id: Some(offer) } => {
                state.finish_offer(offer.id());
                offer.destroy();
            }
            ext_data_control_device_v1::Event::Finished => state.finished = true,
            _ => {}
        }
    }

    event_created_child!(WatchState, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ())
    ]);
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for WatchState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _data: &(),
        connection: &wayland_client::Connection,
        _queue: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => {
                state.offers.insert(id.id(), Vec::new());
            }
            zwlr_data_control_device_v1::Event::Selection { id: Some(offer) } => {
                if let Err(error) = state.receive_wlr(offer, connection) {
                    let _ = state.events.send(ClipboardEvent::Warning(format!(
                        "接收 wlr-data-control 剪贴板失败: {error:#}"
                    )));
                }
            }
            zwlr_data_control_device_v1::Event::PrimarySelection { id: Some(offer) } => {
                state.finish_offer(offer.id());
                offer.destroy();
            }
            zwlr_data_control_device_v1::Event::Finished => state.finished = true,
            _ => {}
        }
    }

    event_created_child!(WatchState, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ())
    ]);
}

pub(super) fn run(
    capture: SyncSender<CaptureTrigger>,
    events: Sender<ClipboardEvent>,
    gate: RecordingGate,
) -> Result<()> {
    let connection =
        wayland_client::Connection::connect_to_env().context("无法连接 Wayland compositor")?;
    let (globals, mut event_queue) =
        registry_queue_init::<WatchState>(&connection).context("无法读取 Wayland globals")?;
    let queue = event_queue.handle();
    let seat: WlSeat = globals
        .bind(&queue, 2..=9, ())
        .context("compositor 未提供 wl_seat")?;

    let (device, backend) =
        if let Ok(manager) = globals.bind::<ExtDataControlManagerV1, _, _>(&queue, 1..=1, ()) {
            (
                WatchDevice::Ext(manager.get_data_device(&seat, &queue, ())),
                "ext-data-control-v1",
            )
        } else {
            let manager: ZwlrDataControlManagerV1 = globals
                .bind(&queue, 1..=2, ())
                .context("compositor 不支持 ext-data-control 或 wlr-data-control")?;
            (
                WatchDevice::Wlr(manager.get_data_device(&seat, &queue, ())),
                "wlr-data-control-v1",
            )
        };

    let mut state = WatchState {
        capture,
        events: events.clone(),
        gate,
        offers: HashMap::new(),
        finished: false,
        backpressure_warned: false,
    };
    event_queue
        .roundtrip(&mut state)
        .context("初始化 data-control device 失败")?;
    let _ = events.send(ClipboardEvent::BackendReady(backend));

    keep_device_alive(&device);
    loop {
        if state.finished {
            bail!("compositor 已结束 data-control device");
        }
        event_queue
            .blocking_dispatch(&mut state)
            .context("data-control 事件循环中断")?;
    }
}

fn keep_device_alive(device: &WatchDevice) {
    match device {
        WatchDevice::Ext(proxy) => {
            let _ = proxy.id();
        }
        WatchDevice::Wlr(proxy) => {
            let _ = proxy.id();
        }
    }
}
