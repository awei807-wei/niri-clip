// SPDX-License-Identifier: GPL-3.0-only

use std::io::Read;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use os_pipe::PipeReader;
use rustix::event::{poll, PollFd, PollFlags, Timespec};
use wl_clipboard_rs::paste;

use crate::content::{select_format, ClipboardContent};

const TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn read_current_content(max_bytes: usize) -> Result<Option<ClipboardContent>> {
    let mime_types = match paste::get_mime_types_ordered(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
    ) {
        Ok(mime_types) => mime_types,
        Err(paste::Error::NoSeats | paste::Error::ClipboardEmpty | paste::Error::NoMimeType) => {
            return Ok(None)
        }
        Err(error) => return Err(error).context("data-control MIME 枚举失败"),
    };
    let Some(format) = select_format(&mime_types) else {
        return Ok(None);
    };
    let (reader, _) = match paste::get_contents(
        paste::ClipboardType::Regular,
        paste::Seat::Unspecified,
        paste::MimeType::Specific(&format.mime_type),
    ) {
        Ok(contents) => contents,
        Err(paste::Error::NoSeats | paste::Error::ClipboardEmpty | paste::Error::NoMimeType) => {
            return Ok(None)
        }
        Err(error) => return Err(error).context("data-control 内容读取失败"),
    };
    Ok(read_pipe_bytes(reader, max_bytes)?.and_then(|bytes| ClipboardContent::new(format, bytes)))
}

pub(super) fn read_pipe_bytes(reader: PipeReader, max_bytes: usize) -> Result<Option<Vec<u8>>> {
    read_pipe_bytes_with_timeout(reader, max_bytes, TRANSFER_TIMEOUT)
}

fn read_pipe_bytes_with_timeout(
    mut reader: PipeReader,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Option<Vec<u8>>> {
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

    Ok(Some(bytes))
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

    use super::read_pipe_bytes_with_timeout;

    #[test]
    fn preserves_arbitrary_binary_bytes_at_limit() {
        let (reader, mut writer) = pipe().unwrap();
        writer.write_all(&[0x00, 0xff, 0x42]).unwrap();
        drop(writer);

        assert_eq!(
            read_pipe_bytes_with_timeout(reader, 3, Duration::from_secs(1)).unwrap(),
            Some(vec![0x00, 0xff, 0x42])
        );
    }

    #[test]
    fn rejects_oversize_binary_without_partial_content() {
        let (reader, mut writer) = pipe().unwrap();
        writer.write_all(&[1, 2, 3, 4]).unwrap();
        drop(writer);

        assert!(
            read_pipe_bytes_with_timeout(reader, 3, Duration::from_secs(1))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn times_out_when_clipboard_owner_never_writes() {
        let (reader, _writer) = pipe().unwrap();
        let error = read_pipe_bytes_with_timeout(reader, 16, Duration::from_millis(10))
            .expect_err("open writer should make the read time out");
        assert!(error.to_string().contains("超时"));
    }
}
