# niri-clip 文字闭环 — 实施规划

## 目标与范围
从空目录交付第一里程碑完整闭环：Wayland 文字变化进入 SQLite，niri 快捷键唤醒真正 layer-shell Overlay，用户搜索并恢复后由守护进程继续提供 clipboard 内容。

## 架构与实现策略
- 单一二进制提供 `daemon/show/toggle/hide/pause/resume/toggle-pause/delete/clear/status` 命令；CLI 客户端只通过 Unix Socket 访问守护进程。
- 守护进程以 GTK 主线程作为状态串行化边界；IPC、clipboard data-control 与读取工作在线程中运行，通过消息队列回到主线程，避免跨线程使用 GTK/SQLite。
- `clipboard` 使用持久 data-control device 订阅 selection 事件并直接读取对应 offer；优先 ext-data-control，回退 wlr-data-control；协议不可用时启用低频哈希轮询并明确上报降级。
- 原生读取通过 os_pipe/rustix 实施 64 项有界队列、5MB 上限和 2 秒超时；wl-clipboard-rs 负责轮询回退和恢复后的 text MIME 所有权。
- 暂停使用原子代际门禁，事件接收、传输完成和主线程落库前均复核，跨暂停/恢复的迟到任务一律失效。
- `storage` 使用 BLAKE3 精确哈希、事务式“删旧插新”语义和单 SQL 截断；DB、目录、socket 权限分别固定为 0600、0700、0600。
- `ui` 为四边锚定的全屏透明 Overlay 宿主，Cyber-Zen 主面板在当前输出中央；显示前读取 `niri msg --json focused-output` 并匹配 GDK connector；所有交互由 GTK 原生键盘控制器完成。

## 领域语言
- “捕获”：读取新 selection 并尝试写入历史。
- “恢复”：将历史原文重新设置为常规 clipboard，之后仍由守护进程提供内容。
- “暂停”：监听仍保持连接，但任何新内容都不读取、不保存。
- “当前输出”：niri `focused-output` 返回的 connector 对应显示器。

## 完成定义
- 首个证明点可复现：复制唯一文字 → `niri-clip show` → Overlay 出现在 focused output 中央 → 查询过滤 → Enter 恢复 → 其他应用读取到完全相同文字。
- 重复内容只保留一条且移动到最新，超限历史自动裁剪；空白原文除全空白外按字节保真。
- 暂停、恢复、删除、清空通过 UI 与 IPC 都能工作，重启后暂停状态保持。
- qaMode 为 deep；重点检查线程边界、Wayland 所有权、IPC 权限、SQL 数据完整性、键盘与状态覆盖。

## 文件结构
- `src/main.rs`、`src/lib.rs`：入口与公共模块。
- `src/daemon.rs`、`src/clipboard.rs`、`src/clipboard/{native,transfer}.rs`、`src/storage.rs`、`src/search.rs`、`src/ui.rs`、`src/ui/{view,signals,presentation}.rs`、`src/ipc.rs`、`src/config.rs`：职责模块。
- `assets/style.css`：GTK CSS token 与组件样式。
- 各模块内 `#[cfg(test)]`：存储、搜索、IPC、捕获门禁、传输和纯 UI 呈现测试。
- `systemd/niri-clip.service`：用户服务。
- `README.md`、`docs/upstream-audit.md`、`LICENSE`、`THIRD_PARTY.md`：交付与合规文档。

## UI / 设计约束
严格遵循 `.helloagents/DESIGN.md` 和用户 `~/.config/quickshell/` 主 Shell 的 Cyber-Zen token。结果面板是唯一主表面；状态用 Stack/Revealer 统一切换；关闭覆盖按钮、Esc、toggle 与点击外部；实际截图检查结果、空、无结果、错误与暂停中的可自动构造状态。

## 风险与验证
- data-control 协议差异：双协议实现并在当前 niri 上查看 layer/clipboard 行为；最终轮询只作为明确降级。
- GTK layer surface 显示器选择：通过 niri focused output + GDK connector 映射，并用 `niri msg layers` 与截图核验。
- clipboard 所有权生命周期：恢复后从独立进程读取，多次读取并在 Overlay 关闭后再次读取。
- GTK 依赖编译成本：先锁定当前 crates.io 版本，持续运行 check/clippy，避免最后集中暴露 API 偏差。
- 回退点：每个模块以单元测试封装；若原生监听编译或运行失败，只替换 clipboard watcher，不改变 storage/UI/IPC 契约。

## 决策记录
- [2026-07-12] 采用 GPL-3.0-only，与行为参考项目 cliphist 的许可方向一致。
- [2026-07-12] 不兼容 cliphist BoltDB/管道协议，只保留“最新优先、预览不损原文、重复移动到最新、数量裁剪”的行为语义。
- [2026-07-12] 不使用外部 `wl-paste --watch`，以原生 data-control 事件保证 Rust 常驻监听；wl-clipboard-rs 负责安全读写和 clipboard 持有。
- [2026-07-12] 当前目录位于脏的上级 Git 仓库内，使用项目本地 `.git-local` 元数据创建隔离检查点。
- [2026-07-12] UI 改为复用本机 Quickshell 的 Cyber-Zen token 与密度；不复制 QML 代码，保留适合剪贴板搜索的 GTK 扁平列表结构。
- [2026-07-12] selection 事件直接消费自身 offer，避免快速 A→B 时重新读取“当前值”造成合并；传输增加有界队列、超时和 Finished 回退。
- [2026-07-12] vendored wayland-scanner 仅应用已进入上游的 quick-xml 0.41 适配，以消除两个已知 RustSec 漏洞。
