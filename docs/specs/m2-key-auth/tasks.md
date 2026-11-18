# M2 任务清单

- [x] T1 数据库迁移 `src-tauri/migrations/0002_ssh_key_secret.sql`：`ssh_key.private_key`
- [x] T2 新增 `src-tauri/src/keys.rs`：`key_list` / `key_import` / `key_delete`
- [x] T3 `src-tauri/src/session.rs`：`HostRow` 补 auth/key，`acquire` 分支，`connect_with_key`
- [x] T4 `src-tauri/src/commands.rs`：`host_save` upsert、`host_delete`、`known_host_list` / `known_host_delete`
- [x] T5 `src-tauri/src/lib.rs`：注册模块与命令
- [x] T6 后端验证：`cargo check --all-targets`
- [x] T7 `src/stores/hosts.ts` + 新增 `src/stores/keys.ts`
- [x] T8 新增 `src/components/host/HostEditor.vue`
- [x] T9 新增 `src/components/host/KeyManager.vue`
- [x] T10 新增 `src/components/host/KnownHosts.vue`
- [x] T11 `QuickConnect.vue` 支持密钥主机（隐藏密码）
- [x] T12 `HostTree.vue` 右键菜单 + 密钥/已知主机入口
- [x] T13 `App.vue` 浮层接线、删除确认、密钥主机直连
- [x] T14 前端验证：`vue-tsc --noEmit`、`vite build`
- [x] T15 手工验证清单（见 spec 第 5 节）：已用临时 russh 验收服务端跑通密钥登录正/负例，验证后清理代码；8.136.100.65 真机确认按用户要求取消
