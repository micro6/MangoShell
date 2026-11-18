# M4 规格：端口转发（本地 / 动态 SOCKS5 / 远程）

> 技术栈沿用：russh 0.65.2 + tauri 2 + Vue 3 + sqlx(SQLite)
> 不新增依赖：SOCKS5 手写最小实现（no-auth + CONNECT），转发复用 `SessionManager` 已认证连接

## 1. 范围

| 能力      | 说明                                                                                           |
| --------- | ---------------------------------------------------------------------------------------------- |
| 本地 `-L` | `bind_host:bind_port` 监听，每条入站连接开 `direct-tcpip` 通道桥接到 `target_host:target_port` |
| 动态 `-D` | 本地 SOCKS5（no-auth，仅 CONNECT，支持 IPv4 / 域名 / IPv6），每条连接按目标开 `direct-tcpip`   |
| 远程 `-R` | 请求服务器监听 `bind_host:bind_port`，服务器回推通道后桥接到本地 `target_host:target_port`     |
| 规则管理  | 新建 / 编辑 / 删除 / 启动 / 停止，落库持久化                                                   |
| 流量统计  | 运行中规则实时显示入/出字节，1 s 推送一次                                                      |
| 降级提示  | `-R` 被服务器拒绝时给出明确原因（`tcpip-forward` denied / 端口占用等）                         |

不做：UDP 转发、SOCKS5 认证、多跳链式转发、规则自动拉起（`autoStart` 仅落库，M4 不自动触发）、调用系统 `ssh.exe` 兜底、断线自动重连。

## 2. 后端设计（Rust）

### 2.1 迁移 `0003_forward_rule.sql`

```sql
CREATE TABLE forward_rule (
  id TEXT PRIMARY KEY,
  host_id TEXT NOT NULL,
  type TEXT NOT NULL,               -- local | dynamic | remote
  bind_host TEXT NOT NULL,
  bind_port INTEGER NOT NULL,
  target_host TEXT,
  target_port INTEGER,
  auto_start INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
```

### 2.2 新增 `src-tauri/src/forward.rs`

```rust
/// 运行中规则的注册表：-R 的回推通道靠它按 (监听地址, 端口) 找到本地目标
pub struct ForwardRegistry {
    rules: RwLock<HashMap<u16, Arc<ForwardTarget>>>, // 以监听端口为键
}

struct ForwardTarget {
    target_host: String,
    target_port: u16,
    counters: Arc<Counters>,     // bytes_in / bytes_out: AtomicU64
    shutdown: CancellationToken 等价物（AtomicBool + Notify）
}

pub struct ForwardManager {
    registry: Arc<ForwardRegistry>,
    running: Mutex<HashMap<String, RunningForward>>, // rule_id → 运行状态
}
```

`RunningForward` 持有：`JoinHandle`（监听循环）、`Arc<Counters>`、实际绑定端口、`Arc<AtomicBool>` 停止标志。

命令（注册进 `lib.rs`，`AppState` 增加 `forwards: Arc<ForwardManager>`）：

| 命令             | 签名要点                                                                           | 行为                                                                                                                       |
| ---------------- | ---------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `forward_list`   | `host_id?: String`                                                                 | 返回 `Vec<ForwardRuleView>`：规则字段 + `status`（`stopped`/`running`/`error`）+ `bytesIn`/`bytesOut` + `error?`           |
| `forward_save`   | `id?, host_id, type, bind_host, bind_port, target_host?, target_port?, auto_start` | 校验：端口 1–65535；`local`/`remote` 必须有 target；类型枚举校验；id 为空新增，否则更新（运行中禁止改名/改端口，需先停止） |
| `forward_delete` | `id`                                                                               | 先停止再删库                                                                                                               |
| `forward_start`  | `id`                                                                               | 按类型启动；失败返回明确错误（含地址、端口、原因）                                                                         |
| `forward_stop`   | `id`                                                                               | 停监听 + 关闭活跃通道；`-R` 额外 `cancel_tcpip_forward`                                                                    |
| `forward_stats`  | `on_stats: Channel<Vec<ForwardStat>>`                                              | 每 1 s 推送全部运行中规则的 `{ id, bytesIn, bytesOut }`；前端丢弃 Channel 后任务自动退出                                   |

### 2.3 三种转发实现

- **`-L`**：`TcpListener::bind(bind_host:bind_port)` 预检并直接复用该 listener；accept 循环 spawn 每连接：
  `handle.channel_open_direct_tcpip(target_host, target_port, "127.0.0.1", 0)` → `channel.into_stream()` ↔ `TcpStream`，用 64 KiB 缓冲手写双向 copy 以便计数。
- **`-D`**：同一 listener，先完成 SOCKS5 握手（方法协商 `0x05 0x00`、请求 `CMD=0x01`、ATYP 支持 0x01/0x03/0x04），成功回 `0x05 0x00 0x00 0x01 0.0.0.0 0`，失败回对应 REP（0x03 网络不可达 / 0x05 连接被拒 / 0x07 命令不支持），再按目标开 `direct-tcpip`。
- **`-R`**：`handle.tcpip_forward(bind_host, bind_port)` 返回实际端口（服务器可分配）；把 `ForwardTarget` 注册进 `ForwardRegistry`。`russh::client::Handler` 新增回调：

```rust
async fn server_channel_open_forwarded_tcpip(
    &mut self, channel, connected_address, connected_port, originator_address, originator_port, reply, session,
) -> Result<(), Self::Error>
```

在 `HostKeyVerifier` 中实现：按 `connected_port` 查 registry，命中则 `reply.accept()` 并把通道桥接到 `TcpStream::connect(target)`；未命中或本地连接失败则 `reply.reject(ChannelOpenFailure::Prohibited)` 并记日志。

### 2.4 接线改动

- `HostKeyVerifier::new(pool, host, port, forwards: Arc<ForwardRegistry>)`；`SessionManager::new(pool, forwards)`。`lib.rs` 先建 `ForwardRegistry`，再建 `SessionManager` 与 `ForwardManager`。
- `AppState { db, db_path, sessions, terminals, sftp, forwards }`。
- 连接断开：规则在下次 `forward_list` 时探活（通道打开失败）→ 置 `error` 并保留错误信息；不自动重启。

### 2.5 统计口径

`bytesOut` = 本地/入站 → 远端方向写入字节；`bytesIn` = 远端 → 本地方向写入字节。每个连接累加到规则级计数器。

## 3. 前端设计（Vue）

### 3.1 `src/stores/forwards.ts`

`rules`, `load(hostId)`, `save(payload)`, `remove(id)`, `start(id)`, `stop(id)`, `subscribeStats()`（`Channel` 订阅，标签卸载时 stop）。

### 3.2 组件

- `src/components/forward/ForwardPane.vue`：按 `tab.hostId` 列出该主机规则；表格列：类型 / 监听 / 目标 / 状态 / 流量（入↑出↓）/ 操作（启动、停止、编辑、删除）；右上「新建隧道」。
- `src/components/forward/ForwardRuleDialog.vue`：类型切换（本地/动态/远程）动态显示目标字段；`bindHost` 默认 `127.0.0.1`，填 `0.0.0.0` 时对话框内红字提示「将暴露到局域网」；端口与必填校验。

### 3.3 接入

- `App.vue`：`kind === "forward"` 渲染 `ForwardPane`；工具栏新增「隧道」按钮，当前标签有 `hostId` 时可用，点击 `openTab({ kind: "forward", hostId, title: `${alias} · 隧道`, state: "connected" })`。
- `TabBar.vue`：forward 标签使用隧道图标（`Waypoints`）。
- 关闭标签只关视图，不停止规则（规则属于主机，可在重新打开时继续看到运行状态并停止）。

## 4. 安全与约束

- 默认仅绑 `127.0.0.1`；`0.0.0.0` 仅提示不拦截（用户明确选择）。
- 不记录转发明文数据，日志只记连接建立/关闭与错误。
- 所有转发复用已认证的 SSH 连接，不新增凭据存储。
- 端口占用在 `start` 时显式报错，不静默换端口。

## 5. 验证

1. `cargo check --all-targets`、`vue-tsc --noEmit`、`vite build`。
2. 真机（8.136.100.65）：
   - `-L`：`127.0.0.1:18022 → 127.0.0.1:22`，`Test-NetConnection` 通；关闭规则后端口释放（再连失败）。
   - `-D`：`curl --socks5-hostname 127.0.0.1:11080 http://www.baidu.com` 返回 200；域名解析走远端。
   - `-R`：本地起 `python -m http.server 8000`，远程监听 `127.0.0.1:19090`，服务器上 `curl 127.0.0.1:19090` 拿到本地内容。
   - 统计：三条规则传输时 `bytesIn/bytesOut` 增长；停止后归零或保持终值。
   - 异常：`-R` 绑定一个已被占用端口，界面展示明确错误。

## 6. 风险

- 服务器 `MaxSessions` / 通道数限制：并发连接多时可能拒绝，错误透传到 UI。
- `-R` 兼容性：部分 sshd 禁用 `tcpip-forward`（`RequestDenied`），按 2.3 给明确提示，不自动兜底。
- Windows 防火墙对 `0.0.0.0` 绑定可能弹窗放行。
- 地址匹配：服务器回推的 `connected_address` 可能与请求的 `bind_host` 文本不同（如 `localhost` vs `127.0.0.1`），因此 registry 以端口为键。
