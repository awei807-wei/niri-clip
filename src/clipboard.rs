// SPDX-License-Identifier: GPL-3.0-only

//! Native Wayland clipboard monitoring and ownership.
//!
//! Each native data-control offer is consumed directly. This preserves rapid
//! clipboard changes instead of re-reading whichever selection happens to be
//! current by the time a worker thread runs.

mod native;
mod transfer;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use os_pipe::PipeReader;
use wl_clipboard_rs::copy;

const POLL_INTERVAL: Duration = Duration::from_millis(450);
const CAPTURE_QUEUE_CAPACITY: usize = 64;

#[derive(Debug)]
pub enum ClipboardEvent {
    Captured { content: String, epoch: u64 },
    BackendReady(&'static str),
    Warning(String),
}

#[derive(Clone, Debug)]
pub struct RecordingGate {
    state: Arc<AtomicU64>,
}

impl RecordingGate {
    pub fn new(paused: bool) -> Self {
        Self {
            state: Arc::new(AtomicU64::new(u64::from(paused))),
        }
    }

    pub fn is_paused(&self) -> bool {
        self.snapshot() & 1 == 1
    }

    pub fn snapshot(&self) -> u64 {
        self.state.load(Ordering::Acquire)
    }

    pub fn accepts(&self, epoch: u64) -> bool {
        epoch & 1 == 0 && self.snapshot() == epoch
    }

    pub fn set_paused(&self, paused: bool) {
        let paused_bit = u64::from(paused);
        let mut current = self.snapshot();
        loop {
            if current & 1 == paused_bit {
                return;
            }
            let next = (current & !1).wrapping_add(2) | paused_bit;
            match self
                .state
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return,
                Err(observed) => current = observed,
            }
        }
    }
}

pub(super) enum CaptureTrigger {
    Native { reader: PipeReader, epoch: u64 },
    Poll { epoch: u64, baseline: bool },
}

impl CaptureTrigger {
    fn epoch(&self) -> u64 {
        match self {
            Self::Native { epoch, .. } | Self::Poll { epoch, .. } => *epoch,
        }
    }
}

pub fn start_monitor(
    events: Sender<ClipboardEvent>,
    gate: RecordingGate,
    max_bytes: usize,
) -> Result<()> {
    let (capture_tx, capture_rx) = mpsc::sync_channel(CAPTURE_QUEUE_CAPACITY);
    let capture_events = events.clone();
    let capture_gate = gate.clone();
    thread::Builder::new()
        .name("niri-clip-reader".to_owned())
        .spawn(move || reader_loop(capture_rx, capture_events, capture_gate, max_bytes))
        .context("无法创建剪贴板读取线程")?;

    thread::Builder::new()
        .name("niri-clip-watcher".to_owned())
        .spawn(move || {
            if let Err(error) = native::run(capture_tx.clone(), events.clone(), gate.clone()) {
                let _ = events.send(ClipboardEvent::Warning(format!(
                    "原生 data-control 监听不可用，已切换到 450ms 轮询: {error:#}"
                )));
                run_poll_fallback(capture_tx, gate);
            }
        })
        .context("无法创建剪贴板监听线程")?;
    Ok(())
}

pub fn copy_text(content: &str) -> Result<()> {
    copy::copy(
        copy::Options::new(),
        copy::Source::Bytes(content.as_bytes().into()),
        copy::MimeType::Text,
    )
    .context("无法取得 Wayland 剪贴板所有权")
}

fn reader_loop(
    capture_rx: mpsc::Receiver<CaptureTrigger>,
    events: Sender<ClipboardEvent>,
    gate: RecordingGate,
    max_bytes: usize,
) {
    let mut last_poll_epoch = gate.snapshot();
    let mut last_polled_hash = None;
    let mut last_error = String::new();

    while let Ok(trigger) = capture_rx.recv() {
        let epoch = trigger.epoch();
        if !gate.accepts(epoch) {
            continue;
        }

        let result = match trigger {
            CaptureTrigger::Native { reader, .. } => transfer::read_pipe_text(reader, max_bytes),
            CaptureTrigger::Poll { baseline, epoch } => {
                let transitioned = epoch != last_poll_epoch;
                last_poll_epoch = epoch;
                let result = transfer::read_current_text(max_bytes);
                if let Ok(Some(content)) = &result {
                    let hash = blake3::hash(content.as_bytes());
                    if baseline || transitioned {
                        last_polled_hash = Some(hash);
                        continue;
                    }
                    if last_polled_hash.as_ref() == Some(&hash) {
                        continue;
                    }
                    last_polled_hash = Some(hash);
                } else if matches!(result, Ok(None)) {
                    last_polled_hash = None;
                }
                result
            }
        };

        if !gate.accepts(epoch) {
            continue;
        }
        match result {
            Ok(Some(content)) => {
                last_error.clear();
                let _ = events.send(ClipboardEvent::Captured { content, epoch });
            }
            Ok(None) => {}
            Err(error) => {
                let message = format!("读取 Wayland 剪贴板失败: {error:#}");
                if message != last_error {
                    last_error.clone_from(&message);
                    let _ = events.send(ClipboardEvent::Warning(message));
                }
            }
        }
    }
}

fn run_poll_fallback(capture: mpsc::SyncSender<CaptureTrigger>, gate: RecordingGate) {
    let mut baseline = true;
    loop {
        let trigger = CaptureTrigger::Poll {
            epoch: gate.snapshot(),
            baseline,
        };
        match capture.try_send(trigger) {
            Ok(()) => baseline = false,
            Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => break,
        }
        thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::RecordingGate;

    #[test]
    fn recording_gate_invalidates_work_across_pause_transitions() {
        let gate = RecordingGate::new(false);
        let initial = gate.snapshot();
        assert!(gate.accepts(initial));

        gate.set_paused(true);
        assert!(gate.is_paused());
        assert!(!gate.accepts(initial));
        let paused = gate.snapshot();
        assert!(!gate.accepts(paused));

        gate.set_paused(false);
        assert!(!gate.is_paused());
        assert!(!gate.accepts(initial));
        assert!(gate.accepts(gate.snapshot()));
    }

    #[test]
    fn redundant_gate_updates_do_not_invalidate_current_work() {
        let gate = RecordingGate::new(false);
        let epoch = gate.snapshot();
        gate.set_paused(false);
        assert_eq!(gate.snapshot(), epoch);
    }
}
