# M1 任务拆解

> 对应规格：[m1-spec.md](./m1-spec.md)。按顺序执行，每步完成后更新 TodoWrite。

- [x] T1 依赖与状态装配
  - `src-tauri/Cargo.toml`：russh 0.65.2（ring/flate2/rsa）、uuid、tokio 扩展特性。
  - `AppState` 增加 `sessions` / `terminals`；`lib.rs` 注册全部命令。
- [x] T2 SSH 会话层
  - `src-tauri/src/session.rs`：连接池、密码认证、30 s keepalive、`known_host` 比对 / TOFU 写入、fail-closed。
- [x] T3 终端通道
  - `src-tauri/src/terminal.rs`：PTY + shell、8 ms / 64 KB 攒批、`term_write / term_resize / term_close`。
- [x] T4 主机命令
  - `src-tauri/src/commands.rs`：`HostRecord`、`host_list`、`host_save`。
- [x] T5 前端终端管理器
  - `src/core/term/manager.ts`（xterm 实例池、Unicode11、WebGL 降级、上行合并、resize 节流、dispose）。
  - `src/components/term/TerminalPane.vue`。
- [x] T6 工作区与主机 UI
  - `src/stores/workspace.ts`（`hostId` 去重）、`src/stores/hosts.ts`。
  - `HostTree.vue`、`TabBar.vue`、`App.vue`、`QuickConnect.vue`。
- [x] T7 验证
  - `cargo check --all-targets` 通过。
  - `pnpm build`（vue-tsc + vite build）通过。
  - 真实 SSH 会话与吞吐基准需在带真实主机的桌面环境手动验证（沙箱内无法执行）。
