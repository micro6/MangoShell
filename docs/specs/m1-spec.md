# M1 规格：单会话终端（密码认证）

> 关联总方案：[ssh-client-solution.md](../../ssh-client-solution.md) 的 5.1 / 5.2 / 9 章与 B.1 / B.2。
> 状态：待评审。批准后再进入实施。

## 1. 目标

打通「快速连接 → SSH 密码认证 → PTY → xterm 渲染 → 键盘上行 / 窗口 resize」的最小闭环，能连真实主机跑 `vim` / `htop` / `tmux`。

## 2. 已核实的技术事实（决定实现方式）

| 事实                              | 结论                                                                                                                                                                 |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 本机无 `cmake`                    | russh 关闭默认 `aws-lc-rs`，改用 `ring` 特性                                                                                                                         |
| russh 0.65.2 API                  | `client::connect` 返回 `Handle<H>`；`Handler::check_server_key(&PublicKeyOrCertificate)` 返回 `bool`；`Channel::split()` 得到 `ChannelReadHalf` + `ChannelWriteHalf` |
| `ChannelWriteHalf` 非 `Clone`     | 用 `Arc<ChannelWriteHalf<Msg>>` 共享给 `term_write` / `term_resize` / `term_close`                                                                                   |
| `ChannelWriteHalf` 方法均 `&self` | `data_bytes / window_change / close` 可直接并发调用                                                                                                                  |
| 下行 `Channel<Vec<u8>>`           | 按总方案先落地，M1 末尾做吞吐基准，不提前切 WebSocket                                                                                                                |
| xterm 6.0.0                       | `@xterm/addon-*` 均已安装，`term.unicode.activeVersion = "11"` 需 `allowProposedApi`                                                                                 |

## 3. 范围

### 3.1 本期做

- 主机表 `host` 的最小读写：列表、新增（快速连接表单）。
- 会话池：一台主机一条 SSH 连接（`user@host:port` 为键），连接复用。
- 认证：仅密码（密码只进入 Rust 内存，不落库、不回传前端）。
- `known_host` 校验：已有记录必须完全匹配，否则拒绝连接；首次连接自动记录（TOFU，M2 再补弹窗确认与指纹展示）。
- 终端通道：PTY + shell、8 ms / 64 KB 攒批下行、16 ms 上行合并、resize 节流 100 ms。
- 前端 xterm 实例池（脱离 Vue 响应式），标签切换用 `v-show` 保留 scrollback。
- 快速连接 Sheet（新建主机 / 已存主机补密码两种模式）。

### 3.2 本期不做

- 密钥 / Agent / 键盘交互认证、keyring、私钥导入。
- 跳板机、断线自动重连、SFTP、端口转发、批量运维。
- known_hosts 首次指纹确认弹窗（M2）、hashed known_hosts 导入。
- 终端搜索 / 日志录制 / 分屏 / 多窗口。
- IPC 背压 ack（先测吞吐再定）。

## 4. 后端设计（src-tauri）

### 4.1 依赖

```toml
russh = { version = "0.65.2", default-features = false, features = ["ring", "flate2", "rsa"] }
uuid = { version = "1", features = ["v4"] }
tokio = { version = "1", features = [
  "rt-multi-thread",
  "sync",
  "time",
  "net",
  "io-util",
  "macros",
] }
```

### 4.2 新增 `src/session.rs`

```rust
pub struct HostConnection {
    pub key: String,                            // user@host:port
    pub handle: russh::client::Handle<HostKeyVerifier>,
}

pub struct SessionManager {
    pool: SqlitePool,
    conns: RwLock<HashMap<String, Arc<HostConnection>>>,
    passwords: RwLock<HashMap<String, String>>, // host_id → 密码（仅内存，M2 迁 keyring）
}

impl SessionManager {
    pub fn new(pool: SqlitePool) -> Self;
    /// 命中复用；未命中则用 password 或内存中已存密码建连并认证
    pub async fn acquire(&self, host_id: &str, password: Option<&str>) -> anyhow::Result<Arc<HostConnection>>;
}
```

- `acquire` 先查 `host` 表（`host/port/username`），读锁命中直接返回；未命中时不持有锁建连，成功后写锁插入。
- `client::Config`：`keepalive_interval = 30s`、`keepalive_max = 3`、`nodelay = true`。
- 认证失败/无密码时返回可读中文错误，前端转 toast。

```rust
pub struct HostKeyVerifier { pool: SqlitePool, host: String, port: u16 }

impl russh::client::Handler for HostKeyVerifier {
    type Error = russh::Error;
    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, russh::Error>;
}
```

- 取 `key.public_key()`，`to_bytes()` 得 blob，`algorithm().as_str()` 得 `key_type`。
- `known_host` 有记录：blob 完全相等才返回 `true`，否则 `warn` 日志并拒绝。
- 无记录：`INSERT ... ON CONFLICT DO NOTHING` 后返回 `true`，并记录 SHA256 指纹日志。
- 数据库异常：fail-closed 返回 `russh::Error::UnknownKey`。

### 4.3 新增 `src/terminal.rs`

```rust
pub struct TermEntry { write: Arc<russh::ChannelWriteHalf<russh::client::Msg>> }

#[derive(Default)]
pub struct TerminalManager { map: RwLock<HashMap<String, Arc<TermEntry>>> }

#[tauri::command]
pub async fn term_open(
    state: State<'_, AppState>, tab_id: String, host_id: String,
    cols: u16, rows: u16, password: Option<String>, on_data: Channel<Vec<u8>>,
) -> Result<(), String>;
```

- 流程：`acquire` → `channel_open_session` → `request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])` → `request_shell(true)` → `split()`。
- 写半部存 `TerminalManager`；读循环 8 ms / 64 KB 攒批后 `on_data.send(Vec<u8>)`。
- `Eof` / `Close` / `None` 结束循环并清理 tab 记录；命令返回即代表会话结束（前端据此置状态）。

```rust
#[tauri::command] pub async fn term_write(state, tab_id: String, data: String) -> Result<(), String>;
#[tauri::command] pub async fn term_resize(state, tab_id: String, cols: u16, rows: u16) -> Result<(), String>;
#[tauri::command] pub async fn term_close(state, tab_id: String) -> Result<(), String>;
```

### 4.4 `src/commands.rs` 扩展

```rust
#[derive(Serialize, sqlx::FromRow)] #[serde(rename_all = "camelCase")]
pub struct HostRecord { id, alias, host, port, username, auth_type }

#[tauri::command] pub async fn host_list(state) -> Result<Vec<HostRecord>, String>;
#[tauri::command] pub async fn host_save(state, alias: String, host: String, port: u16, username: String) -> Result<HostRecord, String>;
```

- `host_save` 校验非空、trim、`uuid::Uuid::new_v4()`、写 `created_at/updated_at`（unix 秒）。
- `AppState` 增加 `sessions: SessionManager`、`terminals: TerminalManager`。
- `lib.rs` 的 `setup` 中构造并 `manage`，`invoke_handler` 注册全部命令。

## 5. 前端设计（src）

| 文件                                              | 职责                                                                                                    |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `src/core/term/manager.ts`                        | xterm 实例池；`mount / dispose / fit / setPassword`；16 ms 上行合并、100 ms resize 节流、WebGL 失败降级 |
| `src/core/term/manager.ts` 内 `Channel<number[]>` | `onmessage` → `term.write(new Uint8Array(data))`                                                        |
| `src/stores/hosts.ts`                             | `host_list` 结果缓存 + `load()`                                                                         |
| `src/stores/workspace.ts`                         | `WorkspaceTab` 增加 `hostId?: string`；同主机同类型标签去重                                             |
| `src/components/term/TerminalPane.vue`            | 单标签容器，`onMounted` 调 `manager.mount`，`onBeforeUnmount` 不销毁（由 App 统一 dispose）             |
| `src/components/host/QuickConnect.vue`            | Sheet 表单；`preset` 存在时为「补密码」模式                                                             |
| `src/components/host/HostTree.vue`                | 读取真实主机列表、搜索过滤、双击 `connect`、新建 `create`                                               |
| `src/components/term/TabBar.vue`                  | `+` 改为 `emit("new-session")`                                                                          |
| `src/App.vue`                                     | 渲染所有 `TerminalPane`（`v-show`）；监听标签删除 → `terminalManager.dispose`；挂 `Toaster`             |

关键行为：

- `manager.setPassword(hostId, pwd)` 在打开标签前调用；`mount` 时消费一次（仅内存）。
- `invoke("term_open", ...)` 的 Promise 结束时：正常 → 标签状态 `lost`；抛错 → `failed` + toast。
- `ResizeObserver` 监听容器，尺寸为 0 时跳过 `fit()`。
- 终端主题色从 `getComputedStyle` 解析 `--background/--foreground/--primary`（浏览器已解析成 rgb）。
- xterm 样式 `@xterm/xterm/css/xterm.css` 在 manager 内引入。

## 6. 验收与验证

1. `cargo check --all-targets`（src-tauri）通过。
2. `pnpm build`（vue-tsc + vite build）通过。
3. 手动（需真实主机，沙箱内无法验证）：
   - 输入主机/端口/用户/密码 → 出现 shell 提示符；
   - `vim` / `htop` / `tmux` 显示正常，窗口缩放后远端的 `$COLUMNS` 同步；
   - 首次连接后 `known_host` 有记录；篡改该记录后重连被拒绝；
   - 关闭标签后远端进程退出，无残留连接。
4. 吞吐记录：`seq 1 3000000` 端到端耗时与内存峰值写入验收记录（M1 末尾，不阻塞功能）。

## 7. 风险

- `Channel<Vec<u8>>` 走 JSON 数组，25 MB 输出可能偏慢；M1 只测不改，超阈值再切本地 WebSocket。
- 无 `cmake` 已规避（ring）；若 ring 在 MSVC 下构建失败，回退启用 vendored OpenSSL 方案需另行评估。
- 密码暂存 Rust 内存属于 M1 过渡方案，M2 必须迁到 OS Keyring。
