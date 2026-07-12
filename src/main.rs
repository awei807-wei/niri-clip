// SPDX-License-Identifier: GPL-3.0-only

use anyhow::{bail, Result};
use niri_clip::config::{AppConfig, AppPaths};
use niri_clip::ipc::{self, Request};

fn main() {
    if let Err(error) = run() {
        eprintln!("niri-clip: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "help".to_owned());
    let paths = AppPaths::discover()?;

    match command.as_str() {
        "daemon" => {
            ensure_no_extra(arguments)?;
            niri_clip::daemon::run(paths, AppConfig::from_env()?)
        }
        "show" => send(&paths, Request::Show, arguments),
        "toggle" => send(&paths, Request::Toggle, arguments),
        "hide" => send(&paths, Request::Hide, arguments),
        "pause" => send(&paths, Request::Pause, arguments),
        "resume" => send(&paths, Request::Resume, arguments),
        "toggle-pause" => send(&paths, Request::TogglePause, arguments),
        "clear" => send(&paths, Request::Clear, arguments),
        "status" => send(&paths, Request::Status, arguments),
        "ping" => send(&paths, Request::Ping, arguments),
        "delete" => {
            let id = arguments
                .next()
                .ok_or_else(|| anyhow::anyhow!("delete 需要历史 ID"))?
                .parse::<i64>()?;
            send(&paths, Request::Delete { id }, arguments)
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("niri-clip {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            print_help();
            bail!("未知命令: {command}")
        }
    }
}

fn send(paths: &AppPaths, request: Request, arguments: impl Iterator<Item = String>) -> Result<()> {
    ensure_no_extra(arguments)?;
    let response = ipc::send(&paths.socket, &request)?;
    if !response.ok {
        bail!(response.message);
    }
    if let (Some(paused), Some(count)) = (response.paused, response.count) {
        println!(
            "{}\t{} item(s)",
            if paused { "paused" } else { "recording" },
            count
        );
    } else if !response.message.is_empty() {
        println!("{}", response.message);
    }
    Ok(())
}

fn ensure_no_extra(mut arguments: impl Iterator<Item = String>) -> Result<()> {
    if let Some(argument) = arguments.next() {
        bail!("多余参数: {argument}");
    }
    Ok(())
}

fn print_help() {
    println!(
        "niri-clip — niri 的 Wayland 富媒体剪贴板历史\n\n\
         用法:\n\
           niri-clip daemon\n\
           niri-clip show|toggle|hide\n\
           niri-clip pause|resume|toggle-pause\n\
           niri-clip delete <id>|clear\n\
           niri-clip status|ping\n\
           niri-clip version"
    );
}
