// SPDX-License-Identifier: GPL-3.0-only

use std::io::Read;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use os_pipe::PipeReader;
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use wl_clipboard_rs::paste;

const TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn read_current_text(max_bytes: usize) -> Result<Option<String>> {
    let (reader, _mime_type) = match paste::get_contents(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Text,
    ) {
        Ok(contents) => contents,
        Err(paste::Error::NoSeats | paste::Error::ClipboardEmpty | paste::Error::NoMimeType) => {
            return Ok(None)
        }
        Err(error) => return Err(error).context("data-control 读取失败"),
    };
    read_pipe_text(reader, max_bytes)
}

pub(super) fn read_pipe_text(reader: PipeReader, max_bytes: usize) -> Result<Option<String>> {
    read_pipe_text_with_timeout(reader, max_bytes, TRANSFER_TIMEOUT)
}

pub(super) fn select_text_mime(mime_types: &[String]) -> Option<String> {
    mime_types
        .iter()
        .find(|mime| mime.as_str() == "text/plain;charset=utf-8")
        .or_else(|| {
            mime_types
                .iter()
                .find(|mime| mime.as_str() == "UTF8_STRING")
        })
        .or_else(|| {
            mime_types
                .iter()
                .find(|mime| wl_clipboard_rs::utils::is_text(mime))
        })
        .cloned()
}

fn read_pipe_text_with_timeout(
    mut reader: PipeReader,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Option<String>> {
    let deadline = Instant::now() + timeout;
    let mut bytes = Vec::with_capacity(max_bytes.min(8192));
    let mut chunk = [0_u8; 8192];

    loop {
        wait_until_readable(&reader, deadline)?;
        let remaining = max_bytes.saturating_add(1).saturating_sub(bytes.len());
        if remaining == 0 {
            return Ok(None);
        }
        let chunk_limit = remaining.min(chunk.len());
        let read = reader
            .read(&mut chunk[..chunk_limit])
            .context("读取剪贴板传输管道失败")?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > max_bytes {
            return Ok(None);
        }
    }

    Ok(String::from_utf8(bytes).ok())
}

fn wait_until_readable(reader: &PipeReader, deadline: Instant) -> Result<()> {
    loop {
        let now = Instant::now();
        if now >= deadline {
            bail!("剪贴板数据传输超时");
        }
        let remaining = deadline.saturating_duration_since(now);
        let timeout = Timespec {
            tv_sec: remaining.as_secs().try_into().unwrap_or(i64::MAX),
            tv_nsec: i64::from(remaining.subsec_nanos()),
        };
        let mut descriptors = [PollFd::new(
            reader,
            PollFlags::IN | PollFlags::HUP | PollFlags::ERR,
        )];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => bail!("剪贴板数据传输超时"),
            Ok(_) => {
                let ready = descriptors[0].revents();
                if ready.contains(PollFlags::NVAL) {
                    bail!("剪贴板数据传输管道无效");
                }
                if ready.intersects(PollFlags::IN | PollFlags::HUP) {
                    return Ok(());
                }
                if ready.contains(PollFlags::ERR) {
                    bail!("剪贴板数据传输管道错误");
                }
            }
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error).context("等待剪贴板数据失败"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::time::Duration;

    use os_pipe::pipe;

    use super::{read_pipe_text_with_timeout, select_text_mime};

    #[test]
    fn accepts_exact_utf8_at_limit() {
        let (reader, mut writer) = pipe().unwrap();
        writer.write_all("中文".as_bytes()).unwrap();
        drop(writer);
        assert_eq!(
            read_pipe_text_with_timeout(reader, 6, Duration::from_secs(1))
                .unwrap()
                .as_deref(),
            Some("中文")
        );
    }

    #[test]
    fn rejects_oversize_and_invalid_utf8() {
        let (reader, mut writer) = pipe().unwrap();
        writer.write_all(b"abcd").unwrap();
        drop(writer);
        assert!(
            read_pipe_text_with_timeout(reader, 3, Duration::from_secs(1))
                .unwrap()
                .is_none()
        );

        let (reader, mut writer) = pipe().unwrap();
        writer.write_all(&[0xff, 0xfe]).unwrap();
        drop(writer);
        assert!(
            read_pipe_text_with_timeout(reader, 3, Duration::from_secs(1))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn prioritizes_explicit_utf8_text_mime() {
        let mimes = vec![
            "text/plain".to_owned(),
            "UTF8_STRING".to_owned(),
            "text/plain;charset=utf-8".to_owned(),
        ];
        assert_eq!(
            select_text_mime(&mimes).as_deref(),
            Some("text/plain;charset=utf-8")
        );
    }

    #[test]
    fn times_out_when_clipboard_owner_never_writes() {
        let (reader, _writer) = pipe().unwrap();
        let error = read_pipe_text_with_timeout(reader, 16, Duration::from_millis(10))
            .expect_err("open writer should make the read time out");
        assert!(error.to_string().contains("超时"));
    }
}
