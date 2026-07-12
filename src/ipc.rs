// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

const MAX_MESSAGE_BYTES: u64 = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum Request {
    Show,
    Toggle,
    Hide,
    Pause,
    Resume,
    TogglePause,
    Delete { id: i64 },
    Clear,
    Status,
    Ping,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
    pub paused: Option<bool>,
    pub count: Option<usize>,
}

impl Response {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            paused: None,
            count: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            paused: None,
            count: None,
        }
    }
}

pub struct Envelope {
    pub request: Request,
    pub response: Sender<Response>,
}

pub struct SocketGuard {
    path: PathBuf,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn bind(path: &Path) -> Result<(UnixListener, SocketGuard)> {
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            bail!("niri-clip 守护进程已在运行");
        }
        fs::remove_file(path).with_context(|| format!("无法移除失效 socket {}", path.display()))?;
    }

    let listener = UnixListener::bind(path)
        .with_context(|| format!("无法绑定 Unix Socket {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok((
        listener,
        SocketGuard {
            path: path.to_owned(),
        },
    ))
}

pub fn start_server(listener: UnixListener, sender: Sender<Envelope>) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("niri-clip-ipc".to_owned())
        .spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        if let Err(error) = handle_connection(stream, &sender) {
                            eprintln!("IPC 请求失败: {error:#}");
                        }
                    }
                    Err(error) => {
                        eprintln!("IPC 监听失败: {error}");
                        break;
                    }
                }
            }
        })
        .expect("无法创建 IPC 线程")
}

pub fn send(path: &Path, request: &Request) -> Result<Response> {
    let mut stream = UnixStream::connect(path)
        .with_context(|| format!("无法连接 niri-clip 守护进程: {}", path.display()))?;
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut line = String::new();
    BufReader::new(stream)
        .take(MAX_MESSAGE_BYTES)
        .read_line(&mut line)?;
    if line.is_empty() {
        bail!("守护进程未返回响应");
    }
    serde_json::from_str(&line).context("守护进程返回了无效响应")
}

fn handle_connection(mut stream: UnixStream, sender: &Sender<Envelope>) -> Result<()> {
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;
    let mut line = String::new();
    BufReader::new(stream.try_clone()?)
        .take(MAX_MESSAGE_BYTES)
        .read_line(&mut line)?;
    let request: Request = serde_json::from_str(&line).context("无效 IPC 请求")?;
    let (response_tx, response_rx) = mpsc::channel();
    sender
        .send(Envelope {
            request,
            response: response_tx,
        })
        .context("守护进程主循环已停止")?;
    let response = response_rx
        .recv_timeout(REQUEST_TIMEOUT)
        .context("守护进程处理 IPC 请求超时")?;
    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{bind, send, start_server, Request, Response};
    use std::sync::mpsc;

    #[test]
    fn request_round_trip_uses_json_line_protocol() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("daemon.sock");
        let (listener, _guard) = bind(&socket).unwrap();
        let (tx, rx) = mpsc::channel();
        let _server = start_server(listener, tx);
        let responder = std::thread::spawn(move || {
            let envelope = rx.recv().unwrap();
            assert_eq!(envelope.request, Request::Status);
            envelope
                .response
                .send(Response {
                    ok: true,
                    message: "ready".to_owned(),
                    paused: Some(false),
                    count: Some(7),
                })
                .unwrap();
        });

        let response = send(&socket, &Request::Status).unwrap();

        assert!(response.ok);
        assert_eq!(response.count, Some(7));
        responder.join().unwrap();
    }
}
