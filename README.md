# niri-clip

`niri-clip` 是面向 Arch Linux + niri 的键盘优先 Wayland 富媒体剪贴板历史。它以单实例 Rust 守护进程持续记录文字、图片、视频与文件引用，并用 GTK4 layer-shell 在当前输出中央显示真正的 Overlay；不启动终端，也不依赖 fuzzel、rofi 或 wofi。

当前版本是 `0.2.0`。

## 已实现

- 原生订阅 `ext-data-control-v1`，不可用时回退 `wlr-data-control-v1`，并直接消费每个 selection offer，快速连续复制不会被合并成最后一项；两者都不可用时明确降级到 450ms 轮询。
- 按 MIME 捕获常规 clipboard 中的 UTF-8 文字、`image/*`、`video/*` 与桌面文件 URI；文件和常见视频文件只记录 URI，不读取文件正文。
- SQLite 类型化持久化，BLAKE3 按内容种类、MIME 和原始字节去重；等价文件 URI 表示会合并。重复内容移动到最新，默认保留 750 条、单条载荷上限 5MB。
- 0.1.0 文字数据库在首次启动时原位迁移；历史列表只加载搜索摘要和最大 72×48 的图片缩略图，完整媒体 BLOB 仅在恢复单条记录时读取。
- 常驻 GTK4 Overlay；显示前读取 niri 的 focused output 并映射到 GDK monitor，中央面板视觉复用本机 Quickshell 的 Cyber-Zen token。
- 混合内容 Unicode 模糊搜索、图片行内缩略图、文件名与 MIME/类型/大小展示；方向键选择、Enter 恢复、Esc 关闭、Shift+Delete 删除。
- 顶栏 `CLOSE ×`、`Esc`、点击面板外均可关闭；`Mod+V` 建议绑定到 `toggle`，再次按下也会收起。
- 暂停记录、删除单条、二次确认清空；暂停状态跨重启保存，暂停期间内容不会在恢复后补录。
- Unix Socket IPC、systemd 用户服务和私有文件权限。

当前不处理 X11、Flatpak Portal 临时文件传输句柄、来源应用、自动 `Ctrl+V`、云同步、托盘或设置 GUI。Overlay 不播放视频；视频文件按 URI 恢复，来源直接提供的原始 `video/*` 仅在单条上限内保存。

## Arch Linux 依赖

```bash
sudo pacman -S --needed base-devel rust gtk4 gtk4-layer-shell sqlite wayland wayland-protocols
```

`noto-fonts` 和 `noto-fonts-cjk` 是推荐字体。`wl-clipboard` 只用于手工冒烟测试，不是运行时依赖。

## 构建与安装

```bash
cargo build --release --locked
install -Dm755 target/release/niri-clip "$HOME/.local/bin/niri-clip"
install -Dm644 systemd/niri-clip.service "$HOME/.config/systemd/user/niri-clip.service"
systemctl --user daemon-reload
systemctl --user import-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE NIRI_SOCKET
systemctl --user enable --now niri-clip.service
```

该服务可由活跃的 `graphical-session.target` 拉起，也可在导入 Wayland 环境后直接启动；因此兼容通过 `niri.service` 管理和直接运行 `niri --session` 两种会话。

确认服务与原生监听后端：

```bash
systemctl --user status niri-clip.service
journalctl --user -u niri-clip.service -n 30 --no-pager
niri-clip status
```

## niri 配置

在 niri 配置的 `binds` 中加入：

```kdl
binds {
    Mod+V { spawn "niri-clip" "toggle"; }
}
```

若 niri 会话不是由 `niri.service` 管理，或用户 systemd 环境没有 Wayland 变量，在 niri 顶层加入：

```kdl
spawn-at-startup "systemctl" "--user" "import-environment" "WAYLAND_DISPLAY" "XDG_CURRENT_DESKTOP" "XDG_SESSION_TYPE" "NIRI_SOCKET"
spawn-at-startup "systemctl" "--user" "start" "niri-clip.service"
```

修改后执行 `niri validate`，再重新加载 niri 配置。

## 操作

| 操作 | 键盘或命令 |
|---|---|
| 显示/再次关闭 | `niri-clip toggle`，建议 `Mod+V` |
| 只显示 | `niri-clip show` |
| 关闭 | `Esc`、顶栏 `CLOSE ×`、点击面板外、`niri-clip hide` |
| 搜索 | 打开后直接输入 |
| 选择 | `↑` / `↓` |
| 恢复并关闭 | `Enter` |
| 删除当前条目 | `Shift+Delete` 或“删除当前” |
| 清空全部 | `Ctrl+Shift+Delete` 或两次点击“清空” |
| 暂停/恢复 | Overlay 底栏按钮或 `niri-clip pause/resume` |

显式 CLI 还支持 `toggle-pause`、`delete <id>`、`clear`、`status` 和 `ping`。

## 配置与数据

环境变量：

- `NIRI_CLIP_MAX_ITEMS`：历史数量上限，必须大于 0，默认 `750`。
- `NIRI_CLIP_MAX_BYTES`：单条剪贴板载荷字节上限，必须大于 0，默认 `5000000`。文件 URI 很小，不受目标文件大小影响；提高该值会允许更大的原始图片/媒体进入 SQLite。
- `NIRI_CLIP_DATA_DIR`：覆盖数据目录，主要用于测试。
- `NIRI_CLIP_RUNTIME_DIR`：覆盖运行目录基路径，主要用于测试。

默认路径：

- 数据库：`$XDG_DATA_HOME/niri-clip/history.sqlite3`，通常是 `~/.local/share/niri-clip/history.sqlite3`。
- Socket：`$XDG_RUNTIME_DIR/niri-clip/daemon.sock`。

数据目录和运行目录强制为 `0700`，数据库和 socket 强制为 `0600`。SQLite 保存原始载荷、MIME、内容种类、搜索摘要、内容哈希、时间和受限图片缩略图，不保存来源应用，也不读取文件 URI 指向的正文。原生传输使用有界队列和 2 秒读取超时；暂停切换会使所有已排队的旧代际任务失效，恢复时不会回填暂停期间最后留下的内容。

从 0.1.0 升级无需手工操作。0.2.0 写入媒体记录后不建议直接降级运行 0.1.0；旧版本会把数据库正文假定为 UTF-8 `TEXT`。

## 行为与协议边界

项目以 [sentriz/cliphist](https://github.com/sentriz/cliphist) 的最新优先、原文保真、去重和裁剪行为为参考，但不兼容其 BoltDB 文件或 `list | decode` 管道协议。IPC 使用私有 JSON Lines Unix Socket，见 [`docs/ipc-protocol.md`](docs/ipc-protocol.md)。完整上游审计见 [`docs/upstream-audit.md`](docs/upstream-audit.md)。

## 验证

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release --locked
```

手工证明点：

1. 执行 `wl-copy '唯一测试文字 alpha 中文'`。
2. 按 `Mod+V`，确认 Overlay 位于当前显示器中央。
3. 输入 `alpha`，确认只保留匹配项。
4. 按 Enter；再执行 `wl-paste --no-newline`，确认输出逐字节一致。

图片：

```bash
wl-copy --type image/png < test.png
niri-clip toggle
# 在 Overlay 选择图片并按 Enter
wl-paste --type image/png > /tmp/niri-clip-restored.png
cmp test.png /tmp/niri-clip-restored.png
```

视频文件引用：在文件管理器复制一个 `.mp4`，打开 Overlay 确认文件名和 `FILES` 类型；按 Enter 后在目标目录粘贴，确认恢复为复制而不是移动。也可用 `wl-copy --type text/uri-list` 与 `wl-paste --type text/uri-list` 做字节级协议检查。

## 故障排查

- `无法连接 niri-clip 守护进程`：检查 `systemctl --user status niri-clip.service`。
- `无法连接 Wayland compositor`：重新导入 `WAYLAND_DISPLAY` 等环境变量后重启服务。
- 日志显示轮询降级：当前 compositor 没有暴露 ext/wlr data-control；niri 正常应提供原生后端。
- 图片只显示 `IMAGE` 标签：内容仍已保存并可恢复，但当前 GdkPixbuf 解码器无法生成缩略图；检查 MIME 是否与真实图片格式一致。
- 大型原始媒体没有进入历史：它超过 `NIRI_CLIP_MAX_BYTES` 或 2 秒传输限制；复制媒体文件本身时应使用文件管理器提供的 URI 列表。
- `niri msg layers` 应显示 namespace `niri-clip`、layer `Overlay`、keyboard interactivity `Exclusive`；关闭后该条目应消失。
- GTK 启动时若错误指向外部主题的 `colors.css`，属于当前 GTK 主题语法问题；项目自带样式错误会显示为 `<data>`。

## 许可证

本项目采用 GPL-3.0-only。上游行为参考、第三方依赖和归属见 [`THIRD_PARTY.md`](THIRD_PARTY.md)。
