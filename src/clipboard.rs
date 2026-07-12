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

use crate::content::{ClipboardContent, ContentFormat, ContentKind};

const POLL_INTERVAL: Duration = Duration::from_millis(450);
const CAPTURE_QUEUE_CAPACITY: usize = 64;

#[derive(Debug)]
pub enum ClipboardEvent {
    Captured {
        content: ClipboardContent,
        epoch: u64,
    },
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
    Native {
        reader: PipeReader,
        format: ContentFormat,
        epoch: u64,
    },
    Poll {
        epoch: u64,
        baseline: bool,
    },
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
    let content = ClipboardContent::new(
        ContentFormat {
            kind: ContentKind::Text,
            mime_type: "text/plain;charset=utf-8".to_owned(),
        },
        content.as_bytes().to_vec(),
    )
    .context("文字剪贴板内容无效")?;
    copy_content(&content)
}

/// Restores a validated payload and serves the appropriate Wayland MIME sources.
pub fn copy_content(content: &ClipboardContent) -> Result<()> {
    copy::copy_multi(copy::Options::new(), copy_sources(content))
        .context("无法取得 Wayland 剪贴板所有权")
}

fn copy_sources(content: &ClipboardContent) -> Vec<copy::MimeSource> {
    let source = |bytes: Vec<u8>, mime_type| copy::MimeSource {
        source: copy::Source::Bytes(bytes.into_boxed_slice()),
        mime_type,
    };
    match content.kind {
        ContentKind::Files => {
            let uri_list = content.uri_list_bytes().unwrap_or_default();
            let gnome = content.gnome_file_bytes().unwrap_or_default();
            vec![
                source(
                    uri_list,
                    copy::MimeType::Specific("text/uri-list".to_owned()),
                ),
                source(
                    gnome,
                    copy::MimeType::Specific("x-special/gnome-copied-files".to_owned()),
                ),
            ]
        }
        ContentKind::Text if is_plain_text_mime(&content.mime_type) => {
            vec![source(content.bytes.clone(), copy::MimeType::Text)]
        }
        ContentKind::Text | ContentKind::Image | ContentKind::Video => vec![source(
            content.bytes.clone(),
            copy::MimeType::Specific(content.mime_type.clone()),
        )],
    }
}

fn is_plain_text_mime(mime_type: &str) -> bool {
    let base = mime_type.split(';').next().unwrap_or(mime_type).trim();
    base.eq_ignore_ascii_case("text/plain")
        || matches!(mime_type, "TEXT" | "STRING" | "UTF8_STRING")
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

        let result = read_trigger(
            trigger,
            max_bytes,
            &mut last_poll_epoch,
            &mut last_polled_hash,
        );

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

fn read_trigger(
    trigger: CaptureTrigger,
    max_bytes: usize,
    last_poll_epoch: &mut u64,
    last_polled_hash: &mut Option<blake3::Hash>,
) -> Result<Option<ClipboardContent>> {
    match trigger {
        CaptureTrigger::Native { reader, format, .. } => {
            transfer::read_pipe_bytes(reader, max_bytes)
                .map(|bytes| bytes.and_then(|bytes| ClipboardContent::new(format, bytes)))
        }
        CaptureTrigger::Poll { baseline, epoch } => {
            let transitioned = epoch != *last_poll_epoch;
            *last_poll_epoch = epoch;
            let result = transfer::read_current_content(max_bytes);
            if let Ok(Some(content)) = &result {
                let hash = content.fingerprint();
                if baseline || transitioned || last_polled_hash.as_ref() == Some(&hash) {
                    *last_polled_hash = Some(hash);
                    return Ok(None);
                }
                *last_polled_hash = Some(hash);
            } else if matches!(result, Ok(None)) {
                *last_polled_hash = None;
            }
            result
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
    use wl_clipboard_rs::copy::{MimeType, Source};

    use crate::content::{ClipboardContent, ContentFormat, ContentKind};

    use super::copy_sources;
    use super::RecordingGate;

    fn content(kind: ContentKind, mime_type: &str, bytes: &[u8]) -> ClipboardContent {
        ClipboardContent::new(
            ContentFormat {
                kind,
                mime_type: mime_type.to_owned(),
            },
            bytes.to_vec(),
        )
        .unwrap()
    }

    #[test]
    fn restore_sources_preserve_media_mime_and_offer_file_compatibility() {
        let image = copy_sources(&content(ContentKind::Image, "image/png", &[0x89, 0x50]));
        assert_eq!(image.len(), 1);
        assert_eq!(
            image[0].mime_type,
            MimeType::Specific("image/png".to_owned())
        );
        assert_eq!(image[0].source, Source::Bytes(vec![0x89, 0x50].into()));

        let files = copy_sources(&content(
            ContentKind::Files,
            "text/uri-list",
            b"file:///tmp/demo.mp4\r\n",
        ));
        let mimes = files
            .iter()
            .map(|source| source.mime_type.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            mimes,
            [
                MimeType::Specific("text/uri-list".to_owned()),
                MimeType::Specific("x-special/gnome-copied-files".to_owned()),
            ]
        );
    }

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
