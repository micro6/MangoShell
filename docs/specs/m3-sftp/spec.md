# M3 规格：SFTP 文件管理（双栏 / 上传下载 / 队列 / 续传 / 权限）

> 技术栈沿用：russh 0.65.2 + tauri 2 + Vue 3 + shadcn-vue
> 新增依赖：`russh-sftp = "3.0.1"`（纯 Rust SFTP 协议层，无 C 依赖；与 russh 的 `Channel::into_stream()` 直接对接）

## 1. 范围

| 能力 | 说明 |
|---|---|
| 双栏 | 左侧本地目录、右侧远端目录，可拖拽分隔条调整宽度 |
| 浏览 | 双击目录进入、上一级、路径栏直填回车跳转、刷新；显示名称/大小/修改时间/权限 |
| 上传下载 | 多选后按钮或双击触发；远端文件下载到本地当前目录，本地文件上传到远端当前目录 |
| 队列 | 单一并发（串行），逐条显示进度/速度/状态，支持取消、失败重试、清理已完成 |
| 续传 | 断点续传自动生效：本地已存在且短于远端则从本地长度续传下载；远端已存在且短于本地则从远端长度续传上传 |
| 权限 | 远端文件/目录 chmod（八进制 3~4 位），删除、重命名、新建目录 |

不做：目录递归传输、远端到远端复制、拖拽上传下载、本地重命名/删除、断点校验（哈希）。

## 2. 后端设计（Rust）

### 2.1 依赖

`src-tauri/Cargo.toml` 增加 `russh-sftp = "3.0.1"`。

### 2.2 新增 `src-tauri/src/sftp.rs`

复用 `SessionManager` 已建立的主机连接：`sftp_open` 内部调用 `sessions.acquire(host_id, password)`，再用 `handle.channel_open_session()` → `request_subsystem(true, "sftp")` → `SftpSession::new(channel.into_stream())`。

```rust
pub struct SftpManager {
    /// tab_id → SFTP 会话
    sessions: RwLock<HashMap<String, Arc<SftpSession>>>,
    /// transfer_id → 取消标志
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
}
```

命令（全部注册进 `lib.rs`，在 `AppState` 增加 `sftp: SftpManager`）：

| 命令 | 签名要点 | 行为 |
|---|---|---|
| `sftp_open` | `tab_id, host_id, password?: String` | 建会话，返回 `{ home: String }`（`canonicalize(".")`） |
| `sftp_close` | `tab_id` | 关闭会话并移除；2 s 超时兜底 |
| `sftp_list` | `tab_id, path` | 返回 `Vec<SftpEntry>`；符号链接标记 `isSymlink`，能解析出目标目录则 `isDir=true` |
| `sftp_mkdir` | `tab_id, path` | `create_dir` |
| `sftp_rename` | `tab_id, from, to` | `rename` |
| `sftp_delete` | `tab_id, path, isDir` | 文件 `remove_file`；目录 `remove_dir`（非空报错，提示先清空） |
| `sftp_chmod` | `tab_id, path, mode: u32` | `set_metadata` 仅设 `permissions = Some(mode & 0o7777)` |
| `sftp_download` | `tab_id, transfer_id, remote, local, on_progress: Channel<TransferProgress>` | 256 KiB 分块读远端写本地；进度节流 150 ms |
| `sftp_upload` | 同上方向相反 | 256 KiB 分块读本地写远端 |
| `sftp_cancel` | `transfer_id` | 置取消标志，传输循环下一块退出并返回「已取消」 |
| `local_home` / `local_list` | `path?: String` | 本地目录浏览（`USERPROFILE` / `HOME`），返回 `{ path, parent, entries }` |

`SftpEntry`（camelCase 序列化）：`name, path, isDir, isSymlink, size: u64, mode: u32, mtime: i64`。
`TransferProgress`：`{ transferred: u64, total: u64 }`。

### 2.3 续传规则（`sftp_download` / `sftp_upload` 内实现）

- 下载：本地不存在 → 从头写；`local_len < remote_size` → 远端 `seek(local_len)`、本地 append；`local_len == remote_size` → 直接返回完成；`>` → 本地截断重写。
- 上传：远端不存在 → 新建；`remote_len < local_size` → 远端 `seek(remote_len)`、本地 `seek(remote_len)`；`==` → 完成；`>` → 远端截断重写。
- 取消/失败保留部分文件，重试即续传。

## 3. 前端设计（Vue）

### 3.1 新增 `src/stores/transfers.ts`

按 sftp 标签维护串行队列：`TransferItem { id, tabId, direction: "upload"|"download", name, localPath, remotePath, size, transferred, speed, status: "pending"|"running"|"done"|"failed"|"cancelled", error? }`。
`enqueue()` 入队并触发 `runNext()`；`runNext()` 用 `invoke` 调 `sftp_upload/download`，`onProgress` 用 `Channel` 更新条目；失败记录错误，重试重置状态重新入队；`cancel()` 调 `sftp_cancel`。

### 3.2 新增组件

- `src/components/sftp/SftpPane.vue`：单标签容器。挂载时 `sftp_open`（tab 关联主机、密码取自会话缓存/连接池），卸载时 `sftp_close`；顶部路径/按钮，主体 `ResizablePanelGroup`（本地 50% / 远端 50%），底部 `TransferQueue` 可折叠。
- `src/components/sftp/FilePane.vue`：单侧文件表。props：`side: "local"|"remote"`、`tabId`；维护当前路径与选中项；双击目录进入，双击文件加入传输队列；路径栏回车跳转；多选（Ctrl）；按钮：上级/刷新/新建目录（仅远端）；右键菜单（远端：下载、重命名、删除、权限；本地：上传、刷新）。
- `src/components/sftp/TransferQueue.vue`：队列表格、进度条（自绘 div）、速度、状态、取消/重试/清理已完成。
- `src/components/sftp/PermissionDialog.vue`：八进制权限编辑（`/^[0-7]{3,4}$/` 校验），仅远端。

文件表用原生 `<table>` + Tailwind 密排样式（现有 UI 组件库无 Table，不额外引入）。

### 3.3 接入

- `src/App.vue`：`term-stack` 内按 `tab.kind` 分支渲染 `TerminalPane`（terminal）与 `SftpPane`（sftp）；关闭标签时仅对 terminal 调 `terminalManager.dispose`。
- 工作区工具栏新增「SFTP」按钮：仅当前标签为已连接的终端标签时可用，点击 `openTab({ kind: "sftp", hostId, title: `${alias} · SFTP`, state: "connected" })`，复用同一主机连接。
- `src/components/term/TabBar.vue`：为 sftp 标签显示文件夹图标（复用现有图标机制即可）。

## 4. 安全与约束

- 传输仅在本机与已认证 SSH 连接之间进行，不新增凭据存储；`sftp_open` 的密码参数语义与 `term_open` 一致（优先用连接池/内存缓存）。
- 本地路径只读浏览、不做删除/覆盖以外的写操作；续传按上述规则可能截断本地文件，UI 在截断重写前于队列中标注「覆盖」。
- 远端删除目录非递归，避免误删整棵树。

## 5. 验证

1. `cargo check --all-targets`、`node node_modules/vue-tsc/bin/vue-tsc.js --noEmit`、`node node_modules/vite/bin/vite.js build`。
2. 传输层临时验收程序（`examples/sftp_accept.rs`，验证后删除）：用 `tokio::io::duplex` 对接 `russh-sftp` 的 server 模块（不可用则起本地 sshd），覆盖：
   - 上传 / 下载正确性（内容一致）；
   - 中断后续传：只传前 1/3 后模拟失败，重试从断点继续，最终内容一致；
   - 取消：`sftp_cancel` 生效且保留部分文件。
3. 手工：远端 100 MB 文件下载吞吐观察；目录浏览、chmod、重命名、删除、多选队列串行执行。

## 6. 风险

- `russh-sftp` 3.0.1 与部分老服务器扩展兼容性（仅用 SFTP v3 基础操作，风险低）。
- Windows 本地路径与远端 `/` 路径拼接需分别处理，路径栏统一用 `/` 展示远端路径。
- 大目录（>5000 项）无虚拟滚动，先全量渲染并在超过阈值时提示，后续里程碑再引入虚拟列表。
