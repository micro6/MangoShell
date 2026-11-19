# MVP：终端断线自动重连

## Context

当前 SSH 终端断开后只会进入 `lost/failed`，用户必须关闭并重新打开标签；原 xterm 内容和操作上下文无法延续。本轮只补 MVP 缺口中的「断线自动重连」，不处理多窗口、终端搜索、日志录制或私钥 Keyring 迁移。

目标是：已建立的终端因网络或远端异常断开时，在原标签内有限次自动重连，保留 scrollback；正常退出、首次连接失败和用户主动关闭不自动重连。

## 推荐方案

由前端 `TerminalManager` 编排重试，后端每次 `term_open` 仍只负责一条终端通道。复用现有 `SessionManager.acquire()` 的失效连接检测、密码内存缓存和私钥认证，不修改连接池结构。

默认策略：最多自动重连 3 次，退避间隔 1 / 2 / 4 秒；连续稳定连接 30 秒后重置次数，避免网络反复闪断造成无限重连。提供「停止重连」和失败后「重新连接」操作。

## 实施步骤

### 1. 后端明确退出原因并支持取消

修改 `src-tauri/src/terminal.rs`：

- 为 `term_open` 增加可序列化返回值，区分 `shellExit`、`disconnected`、`closed`，不再将所有通道结束都当作同一种正常返回。
- `pump()` 记录 `ChannelMsg::ExitStatus / ExitSignal`；收到正常 shell 退出状态后结束为 `shellExit`，网络中断、无退出状态的关闭及异常为 `disconnected`。`Eof` 后等待最终关闭，避免漏掉退出状态。
- 扩展 `TermEntry`，在 acquire 前注册取消通知，写半部允许尚未就绪；`term_open` 用 `tokio::select!` 监听取消和连接/读循环。这样重连在途时关闭标签也能终止 acquire、PTY 或 shell，避免遗留远端进程。
- `term_close` 同时取消在途连接并关闭已建立的终端通道；`term_write/term_resize` 在通道尚未就绪时返回明确错误。清理仍按 `tabId` 执行，前端保证同一标签同时只有一次 `term_open`。
- 只关闭终端自己的 channel，不关闭共享 SSH handle，不自动停止或恢复 SFTP、端口转发。

复用且原则上不修改 `src-tauri/src/session.rs`：`acquire()` 已通过 `handle.is_closed()` 丢弃失效连接；密码认证在首次成功后使用后端内存密码，私钥认证继续读取现有密钥记录，重连仍执行 known_hosts 校验。

### 2. 前端在原 xterm 实例上重连

修改 `src/core/term/manager.ts`：

- 将单次 `term_open` 调用整理为可重复执行的 attempt；每次创建新 `Channel`，复用原 `Terminal`、Fit/ResizeObserver 和 scrollback，不调用 `term.reset()`。
- 为每个标签保存连接代次、是否曾就绪、重试次数及定时器。Channel 消息和 Promise 回调携带代次，旧 attempt 的迟到消息/结果直接忽略，防止污染新会话。
- 仅在「曾收到首包并进入 ready」后发生 `disconnected`，或重连 attempt 失败时继续有限次重试；首次连接失败、`shellExit`、`closed`、主动关闭均不自动重试。
- 断线时清空未发送的上行缓冲并暂时禁用 stdin，首包恢复后重新启用；稳定 30 秒清零重试次数。
- 增加立即重试和停止重连方法。`dispose()` 统一使旧代次失效、清理重试/稳定/输入/resize 定时器并调用 `term_close`，沿用 `App.vue` 现有标签关闭监听。
- 密码仍只在首次 attempt 从前端取出，后续传 `None`，由后端内存缓存完成认证，不新增密码持久化。

### 3. 展示重连状态和必要操作

修改 `src/components/term/TerminalPane.vue`：

- 在终端区域增加轻量状态层，展示「连接已断开，正在第 n/3 次重连」及等待/连接中状态。
- 重连期间复用 workspace 的 `connecting` 状态，首包恢复后切回 `connected`；停止后为 `lost`，最终失败为 `failed`。
- 提供「停止重连」；重试耗尽后提供「重新连接」。只在最终失败时显示错误 toast，避免每次退避重复弹窗。
- `TabBar.vue` 沿用现有脉冲/错误状态样式，`workspace.ts` 和 `App.vue` 的标签复用、关闭销毁流程无需改结构。

### 4. 聚焦测试

- 在 `terminal.rs` 内为退出原因分类增加小型单元测试，覆盖正常 exit-status、signal/无状态关闭和异常断开。
- 使用 Vite+ 现有测试能力为前端重试逻辑补聚焦测试，不引入另一套测试框架；覆盖首败不重试、1/2/4 秒退避、三次耗尽、旧代次回调失效、主动关闭/停止后不再发起 invoke。

## 验证

1. 执行 `cargo test --all-targets`、`cargo check --all-targets`（`src-tauri`）。
2. 执行 `pnpm build`、前端聚焦测试和 `pnpm exec vp check`；不用 Vite+ 格式化 `src-tauri`。
3. 用真实 SSH 主机分别验证密码和密钥认证：
   - 已连接后断网再恢复，原标签自动恢复，历史 scrollback 保留，继续输入和 resize 正常。
   - 输入 `exit`、关闭标签、停止重连均不再发起连接；在退避和连接在途两种阶段关闭都无残留 shell。
   - 持续断网时按 1/2/4 秒重试并在三次后停止，手动重新连接可用；稳定 30 秒后的再次断线重新计数。
   - 同时存在 SFTP/端口转发时，终端重连不误关其管理状态；它们本轮不自动恢复。
   - known_hosts 变化仍拒绝连接，不因自动重连绕过校验。

## 非目标

- 不恢复远端 shell 内已运行的进程、当前目录或环境变量，只恢复终端连接。
- 不自动重连 SFTP、端口转发，不实现无限重试。
- 不做多窗口、终端搜索、日志录制、Keyring 迁移，不恢复 M5/M6。
