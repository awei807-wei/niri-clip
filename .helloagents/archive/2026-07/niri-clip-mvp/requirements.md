# niri-clip 文字闭环 — 需求

确认后冻结，执行阶段不可修改。如需变更必须回到设计阶段重新确认。

## 核心目标
为 Arch Linux + niri 用户提供常驻、快速、键盘优先的 Wayland 文字剪贴板历史：复制文字后通过 `Mod+V` 在当前显示器中央召回、搜索并恢复。

## 功能边界
- 后台监听常规 Wayland clipboard，只保存有效 UTF-8 文字。
- SQLite 持久化，精确内容哈希去重，重复内容移动到最新，历史数量和单条字节数有上限。
- 单实例守护进程持有 GTK4 layer-shell Overlay；`niri-clip show` 通过权限受限的 Unix Socket 唤醒。
- 输入即使用 nucleo-matcher 做 Unicode 模糊搜索；方向键选择；Enter 恢复并关闭；Esc 关闭。
- 支持删除当前条目、清空全部历史和持久化暂停记录。
- 覆盖加载、空历史、无搜索结果、错误、成功、暂停和危险确认状态。
- 提供 systemd 用户服务、Arch 依赖说明、安装流程和 niri `Mod+V` 配置。

## 非目标
- 不处理图片、文件、HTML 富文本和 X11 clipboard。
- 不自动执行 `Ctrl+V`，不使用 uinput 或模拟按键。
- 不做云同步、托盘、设置 GUI 和来源应用排除。
- 不依赖 fuzzel、rofi 或 wofi。
- 不承诺与 cliphist 的 BoltDB 文件格式或 CLI 管道协议二进制兼容。

## 技术约束
- Rust 2021；GTK4 Rust 绑定 + gtk4-layer-shell；SQLite；wl-clipboard-rs；wayland-client；ext/wlr data-control；nucleo-matcher。
- 默认 GPL-3.0-only；只借鉴 cliphist 可观察行为，记录其 GPL-3.0 上游与审计提交；若改写第三方实现必须保留归属。
- 模块按 daemon、clipboard、storage、search、ui、ipc 拆分；单文件和函数遵守项目体积阈值。
- 数据、运行目录与 socket 采用用户私有权限；不记录密钥或来源应用信息。

## 质量要求
- 必须通过 `cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-features`、`cargo build --release`。
- storage、search、IPC 协议和文字过滤边界具备自动化测试。
- 在当前 niri 会话验证捕获、显示、当前输出、搜索/选择、恢复持有、删除、清空与暂停；无法自动操作的项目明确列出。
- UI 冷启动与唤醒不执行网络或外部 picker；常驻显示路径不重新初始化数据库。
