# M4 任务清单

- [x] T1 新增迁移 `src-tauri/migrations/0003_forward_rule.sql`
- [x] T2 新增 `src-tauri/src/forward.rs`：`ForwardRegistry` / `ForwardManager` / 规则 CRUD 命令（`forward_list` / `forward_save` / `forward_delete`）
- [x] T3 `forward.rs`：本地转发 `-L`（listener + direct-tcpip 桥接 + 双向计数）
- [x] T4 `forward.rs`：动态转发 `-D`（手写 SOCKS5 no-auth + CONNECT，IPv4/域名/IPv6）
- [x] T5 `forward.rs` + `session.rs`：远程转发 `-R`（`tcpip_forward`、`HostKeyVerifier` 注入 registry 并实现 `server_channel_open_forwarded_tcpip`）
- [x] T6 `forward.rs`：`forward_start` / `forward_stop` / `forward_stats`（1 s 推送）
- [x] T7 `lib.rs` / `commands.rs`：`AppState` 增加 `forwards`，注册全部转发命令与 `SessionManager::new` 签名调整
- [x] T8 后端验证：`cargo check --all-targets`
- [x] T9 新增 `src/stores/forwards.ts`：列表、保存、删除、启停、统计订阅
- [x] T10 新增 `src/components/forward/ForwardRuleDialog.vue`：类型切换、字段校验、0.0.0.0 提示
- [x] T11 新增 `src/components/forward/ForwardPane.vue`：规则表、状态、流量、启停/编辑/删除
- [x] T12 `App.vue` / `TabBar.vue`：按 `kind === "forward"` 渲染、工具栏「隧道」入口、forward 图标
- [x] T13 前端验证：`vue-tsc --noEmit`、`vite build`
- [x] T14 真机验收：`-L` / `-D` / `-R` 各跑通一组、流量统计、停止后端口释放、`-R` 失败提示
