# 变更日志

## [0.1.0] - 2026-07-12

### 新增
- **[剪贴板]**：原生 ext/wlr data-control offer 直读、UTF-8/大小过滤、有界传输与超时、监听失效轮询回退和常驻内容持有。
- **[存储]**：SQLite 持久化、BLAKE3 精确去重、数量裁剪、删除、清空和暂停状态。
- **[Overlay]**：GTK4 layer-shell 当前输出居中面板、Quickshell Cyber-Zen 视觉、Unicode 模糊搜索、可见关闭入口与完整键盘操作。
- **[IPC]**：权限受限 JSON Lines Unix Socket 和单实例守护进程命令。
- **[交付]**：systemd 用户服务、Arch/niri 配置、GPL-3.0、上游审计和验证说明。

### 安全与可靠性
- **[隐私]**：暂停状态使用捕获代际门禁，在事件接收、传输完成和 SQLite 落库前复核，阻止迟到任务在恢复后写入。
- **[供应链]**：vendored `wayland-scanner 0.31.10` 应用已上游合并的 quick-xml 0.41 适配，规避 `RUSTSEC-2026-0194` 与 `RUSTSEC-2026-0195`。
