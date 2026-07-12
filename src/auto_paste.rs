// SPDX-License-Identifier: GPL-3.0-only

//! Best-effort paste injection after an explicit history restore.

use std::env;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use gtk4::glib;
use serde::Deserialize;

const PASTE_DELAY: Duration = Duration::from_millis(120);
const PASTE_SHORTCUT_ENV: &str = "NIRI_CLIP_PASTE_SHORTCUT";
const CTRL_V_ARGS: [&str; 6] = ["-M", "ctrl", "-k", "v", "-m", "ctrl"];
const CTRL_SHIFT_V_ARGS: [&str; 10] = [
    "-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl",
];
const SHIFT_INSERT_ARGS: [&str; 6] = ["-M", "shift", "-k", "insert", "-m", "shift"];
const TERMINAL_APP_IDS: &[&str] = &[
    "alacritty",
    "com.gexperts.tilix",
    "com.mitchellh.ghostty",
    "com.raggesilver.blackbox",
    "com.system76.cosmic-term",
    "contour",
    "dev.warp.warp",
    "dev.warp.warp-stable",
    "foot",
    "footclient",
    "guake",
    "io.github.raphamorim.rio",
    "kitty",
    "konsole",
    "lxterminal",
    "org.gnome.console",
    "org.gnome.ptyxis",
    "org.gnome.terminal",
    "org.kde.konsole",
    "org.wezfurlong.wezterm",
    "qterminal",
    "sakura",
    "st",
    "tabby",
    "terminator",
    "terminology",
    "tilix",
    "urxvt",
    "warp-terminal",
    "xterm",
    "yakuake",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PasteShortcut {
    CtrlV,
    CtrlShiftV,
    ShiftInsert,
}

impl PasteShortcut {
    fn wtype_args(self) -> &'static [&'static str] {
        match self {
            Self::CtrlV => &CTRL_V_ARGS,
            Self::CtrlShiftV => &CTRL_SHIFT_V_ARGS,
            Self::ShiftInsert => &SHIFT_INSERT_ARGS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PasteMode {
    Auto,
    Disabled,
    Shortcut(PasteShortcut),
}

#[derive(Deserialize)]
struct FocusedWindow {
    app_id: String,
}

/// Hides latency and failures from the UI while allowing focus to return first.
pub(crate) fn schedule(should_paste: impl FnOnce() -> bool + 'static) {
    glib::timeout_add_local_once(PASTE_DELAY, || {
        if !should_paste() {
            return;
        }
        let _ = thread::Builder::new()
            .name("niri-clip-auto-paste".to_owned())
            .spawn(try_paste);
    });
}

fn try_paste() {
    let mode = paste_mode(env::var(PASTE_SHORTCUT_ENV).ok().as_deref());
    let shortcut = match mode {
        PasteMode::Disabled => return,
        PasteMode::Shortcut(shortcut) => shortcut,
        PasteMode::Auto => automatic_shortcut(focused_app_id().as_deref()),
    };
    let _ = wtype_command(shortcut).status();
}

fn focused_app_id() -> Option<String> {
    let output = Command::new("niri")
        .args(["msg", "--json", "focused-window"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| parse_focused_app_id(&output.stdout))?
}

fn parse_focused_app_id(output: &[u8]) -> Option<String> {
    serde_json::from_slice::<FocusedWindow>(output)
        .ok()
        .map(|window| window.app_id)
        .filter(|app_id| !app_id.trim().is_empty())
}

fn paste_mode(value: Option<&str>) -> PasteMode {
    match value.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
        "off" | "none" | "disabled" => PasteMode::Disabled,
        "ctrl-v" | "ctrl+v" => PasteMode::Shortcut(PasteShortcut::CtrlV),
        "ctrl-shift-v" | "ctrl+shift+v" => PasteMode::Shortcut(PasteShortcut::CtrlShiftV),
        "shift-insert" | "shift+insert" => PasteMode::Shortcut(PasteShortcut::ShiftInsert),
        _ => PasteMode::Auto,
    }
}

fn automatic_shortcut(app_id: Option<&str>) -> PasteShortcut {
    if app_id.is_some_and(is_terminal_app) {
        PasteShortcut::CtrlShiftV
    } else {
        PasteShortcut::CtrlV
    }
}

fn is_terminal_app(app_id: &str) -> bool {
    let app_id = app_id.trim().to_ascii_lowercase();
    TERMINAL_APP_IDS.contains(&app_id.as_str())
        || app_id.ends_with(".terminal")
        || app_id.ends_with("-terminal")
}

fn wtype_command(shortcut: PasteShortcut) -> Command {
    let mut command = Command::new("wtype");
    command
        .args(shortcut.wtype_args())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(test)]
mod tests {
    use super::{
        automatic_shortcut, parse_focused_app_id, paste_mode, wtype_command, PasteMode,
        PasteShortcut,
    };

    #[test]
    fn auto_mode_uses_terminal_and_regular_linux_shortcuts() {
        assert_eq!(automatic_shortcut(Some("kitty")), PasteShortcut::CtrlShiftV);
        assert_eq!(
            automatic_shortcut(Some("org.gnome.Terminal")),
            PasteShortcut::CtrlShiftV
        );
        assert_eq!(
            automatic_shortcut(Some("org.gnome.Ptyxis")),
            PasteShortcut::CtrlShiftV
        );
        assert_eq!(automatic_shortcut(Some("firefox")), PasteShortcut::CtrlV);
        assert_eq!(automatic_shortcut(None), PasteShortcut::CtrlV);
    }

    #[test]
    fn environment_override_supports_all_modes_and_silent_disable() {
        assert_eq!(
            paste_mode(Some("ctrl+v")),
            PasteMode::Shortcut(PasteShortcut::CtrlV)
        );
        assert_eq!(
            paste_mode(Some("CTRL-SHIFT-V")),
            PasteMode::Shortcut(PasteShortcut::CtrlShiftV)
        );
        assert_eq!(
            paste_mode(Some("shift-insert")),
            PasteMode::Shortcut(PasteShortcut::ShiftInsert)
        );
        assert_eq!(paste_mode(Some("off")), PasteMode::Disabled);
        assert_eq!(paste_mode(Some("unknown")), PasteMode::Auto);
    }

    #[test]
    fn focused_window_json_and_wtype_arguments_are_stable() {
        assert_eq!(
            parse_focused_app_id(br#"{"app_id":"kitty","title":"shell"}"#).as_deref(),
            Some("kitty")
        );
        assert!(parse_focused_app_id(br#"{"app_id":""}"#).is_none());

        let command = wtype_command(PasteShortcut::CtrlShiftV);
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            arguments,
            ["-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl"]
        );
    }
}
