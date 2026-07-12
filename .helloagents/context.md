# 项目上下文

## 概述
niri-clip 是 Arch Linux + niri 上的 Rust Wayland 文字剪贴板历史：常驻监听、SQLite 持久化，并通过 GTK4 layer-shell Overlay 完成键盘搜索与恢复。

## 技术栈
Rust 2021、Cargo、GTK4/gtk4-layer-shell、wayland-client、ext/wlr data-control、wl-clipboard-rs、os_pipe/rustix、rusqlite、BLAKE3、nucleo-matcher、serde JSON。测试使用 Rust 内置测试框架和 tempfile。

## 架构
`clipboard::native` 订阅 selection 并直接消费各自的 Wayland offer → `clipboard::transfer` 以有界队列、大小上限和超时读取 UTF-8 → GTK 主线程经暂停代际复核后串行写入 `storage` → `ipc` 或 UI 触发 `ui` 显示 → `search` 过滤 → 选择后 `clipboard` 重新取得所有权。GTK 与 SQLite 只在主线程访问；IPC 和 Wayland 事件通过 mpsc 传递。

## 领域语言
- **捕获**：读取新 selection 并尝试写入历史；避免用语：同步。
- **恢复**：把历史原文设为常规 clipboard 并持续提供；避免用语：粘贴。
- **暂停**：保持监听连接但不读取、不保存新内容；每次切换使旧捕获代际失效，恢复后不回填暂停期间内容。
- **当前输出**：niri `focused-output` 返回的 connector，不等同于鼠标所在输出。
- **Overlay**：niri layer `Overlay` 的 layer-shell 表面，不是普通可平铺窗口。

## 目录结构
- `src/`：daemon、clipboard（native/transfer）、storage、search、ui、ipc、config 和入口。
- `assets/`：GTK CSS。
- `systemd/`：用户服务。
- `docs/`：上游审计和 IPC 协议。
- `.helloagents/plans/`、`.helloagents/archive/`：方案生命周期。

## 模块文档
各模块的公共契约位于对应 `src/*.rs`；第一里程碑未创建额外模块知识文件。

## 最近变更
见 [CHANGELOG.md](CHANGELOG.md)
