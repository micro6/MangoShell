# M3 任务清单

- [x] T1 `src-tauri/Cargo.toml` 增加 `russh-sftp = "3.0.1"` 并确认 `Channel::into_stream()` 对接编译通过
- [x] T2 新增 `src-tauri/src/sftp.rs`：`SftpManager`（会话表 + 取消标志）、`sftp_open` / `sftp_close`
- [x] T3 `sftp.rs`：`sftp_list` / `sftp_mkdir` / `sftp_rename` / `sftp_delete` / `sftp_chmod`
- [x] T4 `sftp.rs`：`sftp_download` / `sftp_upload`（256 KiB 分块、进度节流、续传规则、取消）
- [x] T5 `sftp.rs`：`local_home` / `local_list`
- [x] T6 `src-tauri/src/lib.rs`：`AppState` 增加 `sftp`，注册全部 SFTP 命令
- [x] T7 后端验证：`cargo check --all-targets`
- [x] T8 新增 `src/stores/transfers.ts`：串行队列、进度/速度、取消、重试
- [x] T9 新增 `src/components/sftp/FilePane.vue`：双栏通用文件表、导航、多选、右键菜单
- [x] T10 新增 `src/components/sftp/PermissionDialog.vue`
- [x] T11 新增 `src/components/sftp/TransferQueue.vue`
- [x] T12 新增 `src/components/sftp/SftpPane.vue`：双栏 + Resizable + 底部队列
- [x] T13 `src/App.vue`：按 tab.kind 渲染 SftpPane、工具栏 SFTP 入口、关闭标签释放
- [x] T14 前端验证：`vue-tsc --noEmit`、`vite build`
- [x] T15 传输验收：临时 duplex 验收程序验证上传/下载/续传/取消，通过后删除示例代码
