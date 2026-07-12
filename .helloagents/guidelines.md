# 项目约定

## 编码风格
- GTK 与 rusqlite 只能由主线程访问；工作线程只能通过消息类型传递拥有所有权的数据。
- Wayland 正常路径必须先用事件订阅，轮询只能作为有日志的协议故障回退。
- 暂停期间不得读取正文；恢复暂停时不得主动捕获当前 selection。
- UI 文字预览可以规范化空白和控制字符，`HistoryItem.content` 不可修改。

## 命名规范
- `capture/捕获` 只表示记录 clipboard，`restore/恢复` 只表示重新取得 clipboard 所有权；禁止把恢复命名为 paste。
- IPC command 使用 kebab-case，Rust enum variant 使用 PascalCase。

## Git 工作流
当前目录嵌套在用户主目录的上级仓库中，必须使用 `.git-local` 作为隔离 Git 元数据；禁止对 `/home/shiyi` 上级仓库执行 add/commit。提交信息使用 Conventional Commits。

## 测试
- 模块单元测试覆盖 storage、search、IPC、UTF-8 过滤与纯 UI 格式函数。
- 交付门禁为 fmt、Clippy `-D warnings`、全特性测试和 release locked build。
- layer-shell、当前输出和 clipboard 所有权必须在真实 niri/Wayland 会话冒烟。

