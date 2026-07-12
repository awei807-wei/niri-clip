# sentriz/cliphist 上游审计

## 审计基线

- 仓库：`https://github.com/sentriz/cliphist.git`
- 审计提交：`25cc3e4affb6d24398cbcb2f42d8e8cf9cf62823`
- 提交日期：2026-06-08
- 提交说明：`fix: create db with 0600 permissions`
- 许可证：GNU GPL Version 3

已审阅 `cliphist.go`、`cliphist_test.go`、全部 `testdata/*.txtar`、`readme.md`、`contrib/cliphist.service` 和 `LICENSE`。

## 采用的可观察行为

- 历史按最新优先列出。
- 存储原始内容，预览的空白折叠不改变恢复内容。
- 空白内容不进入历史。
- 重复内容删除旧记录并作为最新记录重新插入。
- 记录数量和单条大小有上限，超限时删除最旧内容。
- 删除单条和清空全部是明确操作。
- 数据目录与数据库使用用户私有权限。
- 敏感/暂停状态不得写入历史；niri-clip 用持久暂停开关实现首个里程碑的隐私边界。

## 明确未采用

- BoltDB 数据格式和 bucket/sequence 实现。
- `store/list/decode/delete/delete-query/wipe/compact` CLI 管道协议。
- `"<id>\t<preview>"` picker 输出格式。
- 图片识别、二进制内容和 `xdg-mime` 行为。
- 外部 `wl-paste --watch`、dmenu、rofi、wofi、fuzzel 集成。
- Go 源码结构、函数实现或测试夹具文本。

## 新实现差异

niri-clip 使用 SQLite 和 BLAKE3 全历史精确去重，不限制为上游的最近 N 条去重窗口。它把 picker、搜索、clipboard 恢复和内容持有统一放进常驻 Rust 进程，并以私有 Unix Socket 控制 GTK4 layer-shell Overlay。因此它追随行为语义，但不声称文件或命令协议兼容。

## 代码来源与许可处理

没有复制或逐行翻译 cliphist 的 Go 代码；上游只用于确定可观察行为和测试边界。项目仍采用 GPL-3.0-only，并在 README、本文和 THIRD_PARTY 中保留上游链接、提交和许可证说明。

`src/clipboard.rs` 的 ext/wlr 双协议分派基于公开 Wayland 协议和 `wl-clipboard-rs` 的公共 API 模型重新实现；项目直接依赖 `wl-clipboard-rs` 读取与持有内容。该依赖采用 MIT OR Apache-2.0，归属已记录。

