# 第三方软件与归属

## 行为参考

- [sentriz/cliphist](https://github.com/sentriz/cliphist)，审计提交 `25cc3e4affb6d24398cbcb2f42d8e8cf9cf62823`，GPL-3.0。未复制或逐行翻译源码；行为审计见 `docs/upstream-audit.md`。

## 直接 Rust 依赖

版本由 `Cargo.lock` 固定：

| 组件 | 版本 | 许可证 | 项目 |
|---|---:|---|---|
| anyhow | 1.0.103 | MIT OR Apache-2.0 | dtolnay/anyhow |
| blake3 | 1.8.5 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | BLAKE3-team/BLAKE3 |
| dirs | 6.0.0 | MIT OR Apache-2.0 | soc/dirs-rs |
| gtk4 | 0.11.4 | MIT | gtk-rs/gtk4-rs |
| gtk4-layer-shell | 0.8.0 | MIT | pentamassiv/gtk4-layer-shell-gir |
| nucleo-matcher | 0.3.1 | MPL-2.0 | helix-editor/nucleo |
| os_pipe | 1.2.3 | MIT | oconnor663/os_pipe.rs |
| rusqlite | 0.39.0 | MIT | rusqlite/rusqlite |
| rustix | 1.1.4 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | bytecodealliance/rustix |
| serde / serde_json | 1.0.228 / 1.0.150 | MIT OR Apache-2.0 | serde-rs |
| thiserror | 2.0.18 | MIT OR Apache-2.0 | dtolnay/thiserror |
| wayland-client / wayland-protocols / wayland-protocols-wlr | 0.31.14 / 0.32.13 / 0.3.12 | MIT | smithay/wayland-rs |
| wl-clipboard-rs | 0.9.3 | MIT OR Apache-2.0 | YaLTeR/wl-clipboard-rs |

传递依赖、精确校验和与完整来源记录在 `Cargo.lock` 和 crates.io 包元数据中。项目源码中的 SPDX 标识只声明本项目文件的 GPL-3.0-only，不改变依赖各自许可证。

## 运行时工具

恢复操作不调用外部键盘注入工具；目标应用中的粘贴由用户手动完成。

## Vendored 安全补丁

`vendor/wayland-scanner/` 是 crates.io `wayland-scanner 0.31.10` 的 MIT 许可源码，原许可证保留在 `vendor/wayland-scanner/LICENSE.txt`。它只包含两项已进入 Smithay/wayland-rs 上游的兼容变更：

- `ec2d932855593d48aa83c76820f3efbcfea86d39`：适配 quick-xml 新 API，将 `xml_content()` 改为 `xml10_content()`。
- `d07c4f91f28b42e5a485823ffd9d8d5a210b1053`：将 quick-xml 提升到 0.41。

这一补丁规避 quick-xml 0.39.4 的 `RUSTSEC-2026-0194` 与 `RUSTSEC-2026-0195`。2026-07-12 使用 `cargo-audit 0.22.2` 扫描锁定的 137 个 crate，结果为 0 个已知漏洞；vendored scanner 的 5 个单元测试及主项目测试均通过。
