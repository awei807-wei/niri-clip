// SPDX-License-Identifier: GPL-3.0-only

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

pub const DEFAULT_MAX_ITEMS: usize = 750;
pub const DEFAULT_MAX_BYTES: usize = 5_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppConfig {
    pub max_items: usize,
    pub max_bytes: usize,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            max_items: parse_positive_env("NIRI_CLIP_MAX_ITEMS", DEFAULT_MAX_ITEMS)?,
            max_bytes: parse_positive_env("NIRI_CLIP_MAX_BYTES", DEFAULT_MAX_BYTES)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub runtime_dir: PathBuf,
    pub socket: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let data_dir = match env::var_os("NIRI_CLIP_DATA_DIR") {
            Some(path) => PathBuf::from(path),
            None => dirs::data_dir()
                .context("无法确定 XDG 数据目录")?
                .join("niri-clip"),
        };

        let runtime_base = match env::var_os("NIRI_CLIP_RUNTIME_DIR") {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(env::var_os("XDG_RUNTIME_DIR").context("缺少 XDG_RUNTIME_DIR")?),
        };
        if !runtime_base.is_absolute() {
            bail!("运行目录必须是绝对路径: {}", runtime_base.display());
        }

        let runtime_dir = runtime_base.join("niri-clip");
        ensure_private_dir(&data_dir)?;
        ensure_private_dir(&runtime_dir)?;

        Ok(Self {
            database: data_dir.join("history.sqlite3"),
            socket: runtime_dir.join("daemon.sock"),
            data_dir,
            runtime_dir,
        })
    }
}

pub fn ensure_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("无法创建目录 {}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("无法设置目录权限 {}", path.display()))?;
    Ok(())
}

fn parse_positive_env(name: &str, default: usize) -> Result<usize> {
    let Some(value) = env::var_os(name) else {
        return Ok(default);
    };
    let value = value
        .into_string()
        .map_err(|_| anyhow::anyhow!("{name} 不是有效 UTF-8"))?;
    let parsed = value
        .parse::<usize>()
        .with_context(|| format!("{name} 必须是正整数"))?;
    if parsed == 0 {
        bail!("{name} 必须大于 0");
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::ensure_private_dir;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_directory_uses_owner_only_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("private");

        ensure_private_dir(&path).unwrap();

        let mode = path.metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
