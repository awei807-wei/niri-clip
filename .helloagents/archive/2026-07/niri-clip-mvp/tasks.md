# niri-clip 文字闭环 — 任务分解

## 拆分原则
- 默认按端到端垂直切片拆分：每个任务交付一个可验证行为，而不是单独交付某一层。
- `AFK` 表示代理可独立完成；`HITL` 表示需要用户决策、外部凭据、人工视觉确认或手动验收。
- 厚任务必须继续拆小；横向前置任务只在确有技术依赖时保留。

## 任务列表
- [√] 任务1（AFK）：克隆并审计 cliphist 行为、测试与 GPL-3.0 许可证（依赖：无；涉及文件：`/tmp/cliphist-upstream`、`docs/upstream-audit.md`；完成标准：固定提交并形成实现边界；验证方式：Git 提交和源码审计）。
- [√] 任务2（AFK）：建立 SQLite 文字历史、精确去重、裁剪、删除、清空与暂停持久化（依赖：任务1；涉及文件：`src/storage.rs`；完成标准：存储行为单测通过；验证方式：`cargo test storage`）。
- [√] 任务3（AFK）：建立 Unicode 模糊搜索和可读预览（依赖：任务2；涉及文件：`src/search.rs`；完成标准：中文、大小写、无结果和排序测试通过；验证方式：`cargo test search`）。
- [√] 任务4（AFK）：建立权限受限 Unix Socket 协议与单实例边界（依赖：任务2；涉及文件：`src/ipc.rs`、`src/main.rs`；完成标准：客户端命令可控守护进程；验证方式：IPC 集成测试与运行态命令）。
- [√] 任务5（AFK）：建立 ext/wlr data-control 监听、UTF-8 过滤与 clipboard 持有（依赖：任务2；涉及文件：`src/clipboard.rs`；完成标准：niri 中复制可捕获、恢复后可多次读取；验证方式：单测 + Wayland 冒烟）。
- [√] 任务6（AFK）：建立 GTK4 layer-shell Overlay、当前输出选择、搜索与完整键盘操作（依赖：任务2-5；涉及文件：`src/ui.rs`、`src/daemon.rs`、`assets/style.css`；完成标准：全部界面状态与交互闭环可用；验证方式：niri layer 检查、截图与键盘冒烟）。
- [√] 任务7（AFK）：补齐 systemd、Arch/niri 配置、合规与使用文档（依赖：任务4-6；涉及文件：`systemd/`、`README.md`、`LICENSE`、`THIRD_PARTY.md`、`docs/`；完成标准：按文档可安装启动；验证方式：命令审阅与服务文件校验）。
- [√] 任务8（AFK）：执行格式化、Clippy、全量测试、release 构建与真实 niri 验收（依赖：任务2-7；涉及文件：全项目；完成标准：自动检查全绿，交互限制有证据；验证方式：合同指定命令和冒烟记录）。
- [√] 任务9（AFK）：归档方案、更新状态与创建隔离本地 Git 检查点（依赖：任务8；涉及文件：`.helloagents/`、`.git-local`；完成标准：工作树清洁且提交可查询；验证方式：隔离 Git status/log）。

## Codex /goal 执行入口
不适用；当前对话已获得直接执行到完成授权。

## 进度
全部任务已完成并通过自动检查、真实 niri 验收与隔离 Git 检查点验证。
