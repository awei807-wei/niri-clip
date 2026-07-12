# niri-clip IPC 协议

## 传输

守护进程监听 `$XDG_RUNTIME_DIR/niri-clip/daemon.sock`。目录权限为 `0700`，socket 权限为 `0600`。每个连接只处理一个 UTF-8 JSON 请求和一个 JSON 响应；消息以换行结束，单条上限 16KiB，读写和主循环响应超时均为 3 秒。

## 请求

请求使用 `command` 判别字段：

```json
{"command":"show"}
{"command":"toggle"}
{"command":"hide"}
{"command":"pause"}
{"command":"resume"}
{"command":"toggle-pause"}
{"command":"delete","id":42}
{"command":"clear"}
{"command":"status"}
{"command":"ping"}
```

## 响应

```json
{
  "ok": true,
  "message": "ready",
  "paused": false,
  "count": 42
}
```

`paused` 和 `count` 只在状态响应中有值，其余响应为 `null`。`ok=false` 表示请求已到达守护进程但行为失败；连接、解码或超时错误由 CLI 直接报告。

## 与 cliphist 的关系

该协议不是 cliphist `list/decode/delete/wipe` 管道协议的重实现。niri-clip 在单一常驻进程内保持数据库、GTK UI 和 clipboard 所有权，Unix Socket 只传递控制命令，不传输历史正文。

