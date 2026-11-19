# M2 规格：密钥认证 + 主机/密钥管理 + known_hosts 管理

> 状态：待评审
> 前置：M0/M1 已完成（单会话终端、密码认证、TOFU）
> 约束：中文注释；不过度设计；不新增文档以外的产物

## 1. 范围

| #   | 能力             | 验收标准                                                                      |
| --- | ---------------- | ----------------------------------------------------------------------------- |
| R1  | 密钥导入         | 选择本地 OpenSSH 私钥文件（含带口令的加密私钥），导入后展示指纹；口令不落库   |
| R2  | 密钥管理界面     | 列表展示名称/类型/指纹；删除（被主机引用时阻止）                              |
| R3  | 密钥登录         | 主机 auth_type=key 时，使用所选密钥完成 SSH 公钥认证；RSA 使用服务端最优 hash |
| R4  | 主机管理         | 新建/编辑/删除主机；可切换密码/密钥认证并绑定密钥                             |
| R5  | known_hosts 管理 | 列表展示 host:port / 类型 / SHA256 指纹；删除后该主机恢复 TOFU 首次信任流程   |

非目标：SSH Agent 认证、`~/.ssh/config` 导入、端口转发、SFTP（留到后续里程碑）。

## 2. 安全与存储决策

- 私钥正文存放在 **OS Keyring**（Windows Credential Manager / macOS Keychain / Linux Secret Service），SQLite 的 `ssh_key` 只保留元数据，`secret_ref` 记录 Keyring 条目名。导入加密私钥时用口令解密后保存**已解密**的 OpenSSH PEM；口令本身不保存，连接时无需再次输入。
- Windows 单条凭据上限 2560 字节，RSA 4096 的 PEM 会超限，正文按 2000 字节分块写入，首块携带块数头部。
- 旧版本留在 `ssh_key.private_key` 的正文在启动时迁移到 Keyring，成功后清空该列；迁移失败保留原值并在下次启动重试，连接时仍可回退读取。
- 私钥正文**不出现在任何 IPC 返回值**中；`KeyRecord` 只含元数据与公钥。
- 密码仍只存 Rust 进程内存（M1 现状），不落库。
- known_hosts 删除即删除本地信任记录，下次连接重新 TOFU 写入。

## 3. 后端改动（Rust）

### 3.1 新增迁移 `src-tauri/migrations/0002_ssh_key_secret.sql`

```sql
ALTER TABLE ssh_key ADD COLUMN private_key TEXT NOT NULL DEFAULT '';
```

### 3.2 新增 `src-tauri/src/keys.rs`

- `KeyRecord { id, name, keyType, publicKey, fingerprint, createdAt }`（`camelCase`，无私钥字段）。
- `key_list(state)`: 读取元数据，按 `created_at DESC`。
- `key_import(state, name, path, passphrase)`:
  1. `std::fs::read_to_string(path)`；
  2. `PrivateKey::from_openssh`，失败返回「仅支持 OpenSSH 格式私钥」；
  3. `is_encrypted()` 时必须提供 `passphrase`，`decrypt()` 失败返回「私钥口令不正确」；
  4. `public_key().to_openssh()`、`fingerprint(HashAlg::Sha256)`、`algorithm().as_str()`；
  5. `to_openssh(LineEnding::LF)` 得到明文 PEM 写入 OS Keyring（`src-tauri/src/secrets.rs`）；
  6. 指纹已存在则拒绝（「该密钥已导入」）；`secret_ref` 写 `keyring:{id}`，`private_key` 留空。
- `key_delete(state, id)`: 若 `host.key_id` 仍引用则返回错误并给出主机数；否则先删 Keyring 条目再删元数据。

### 3.3 修改 `src-tauri/src/session.rs`

- `HostRow` 增加 `auth_type: String`、`key_id: Option<String>`，查询同步补列。
- `acquire(host_id, password)` 按 `auth_type` 分支：
  - `key`：要求 `key_id` 存在，按 `secret_ref` 从 OS Keyring 读 PEM（`db:` 旧引用回退读 `ssh_key.private_key`），解析后走 `connect_with_key`；失败信息为「密钥认证失败：请确认公钥已加入远端 authorized_keys」；
  - 其他：沿用密码分支与内存密码缓存。
- 新增 `connect_with_key(host, port, username, PrivateKey)`：复用 `client::connect`，RSA 时先 `best_supported_rsa_hash()`，再 `authenticate_publickey(username, PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg))`。
- 抽出 `fn client_config() -> Arc<client::Config>` 供两种认证共用（keepalive 30s / keepalive_max 3 / nodelay）。

### 3.4 修改 `src-tauri/src/commands.rs`

- `HostRecord` 增加 `key_id: Option<String>`，`host_list` 查询补 `key_id`。
- `host_save` 改为 upsert：
  `host_save(id: Option<String>, alias, host, port, username, auth_type: Option<String>, key_id: Option<String>)`
  - `auth_type` 仅允许 `password | key`，默认 `password`；`key` 必须给 `key_id`；
  - `id` 为空走 INSERT，否则 UPDATE 并检查 `rows_affected`。
- 新增 `host_delete(id)`。
- 新增 `KnownHostRecord { host, port, keyType, fingerprint, addedAt }`；
  `known_host_list` 读 `key_blob` 后用 `PublicKey::from_bytes(...).fingerprint(HashAlg::Sha256)` 计算指纹；
  `known_host_delete(host, port, key_type)`。

### 3.5 修改 `src-tauri/src/lib.rs`

- 注册 `mod keys;` 与全部新命令：`key_list/key_import/key_delete/host_delete/known_host_list/known_host_delete`。

## 4. 前端改动（Vue）

### 4.1 `src/stores/hosts.ts`

- `HostRecord` 增加 `authType: string`、`keyId: string | null`。
- 新增 `save(payload)`（含可选 `id`）与 `remove(id)`，成功后刷新列表。

### 4.2 新增 `src/stores/keys.ts`

- `KeyRecord` 接口 + `load/importKey/remove`，对应 `key_list/key_import/key_delete`。

### 4.3 新增 `src/components/host/HostEditor.vue`

- Sheet 表单：别名、主机、端口、用户名、认证方式（密码/密钥）、密钥下拉（认证方式=密钥时显示，数据来自 keys store）。
- 提交时调用 `hosts.save`，`preset` 存在即编辑；父组件负责关闭与刷新。

### 4.4 新增 `src/components/host/KeyManager.vue`

- Dialog 列表：名称、类型、指纹（等宽、截断 + 复制公钥按钮）；删除按钮带 AlertDialog 确认。
- 导入区：`@tauri-apps/plugin-dialog` 的 `open` 选文件 → 填名称（默认文件名）/口令（可选）→ 调 `key_import`；失败 toast。

### 4.5 新增 `src/components/host/KnownHosts.vue`

- Dialog 列表：`host:port`、key 类型、SHA256 指纹；删除按钮；空状态提示「首次连接会自动记录」。

### 4.6 修改 `src/components/host/QuickConnect.vue`

- `preset.authType === "key"` 时隐藏密码输入，文案提示「使用已配置的密钥认证」，提交时 `password` 为 `undefined`。
- 新建（无 preset）仍为密码认证，保持 M1 行为。

### 4.7 修改 `src/components/host/HostTree.vue`

- 新增 emit：`edit(host)`、`delete(host)`、`manage-keys`、`manage-known-hosts`。
- 列表行：单击选中态，双击 `connect`，右键弹出自绘轻量菜单（连接/编辑/删除，fixed 定位 + 点击遮罩关闭）。
- 侧栏底部两个入口：「密钥管理」「已知主机」。

### 4.8 修改 `src/App.vue`

- 接线 HostEditor（新建/编辑）、KeyManager、KnownHosts 三个浮层。
- 删除主机：AlertDialog 确认 → `hosts.remove` → 关闭该主机标签。
- 双击主机：`authType === "key"` 直接开标签连接（不弹密码）；否则仍走 QuickConnect 密码流程。

## 5. 验证

1. `cd src-tauri && cargo check --all-targets` 通过。
2. 根目录 `node node_modules/vue-tsc/bin/vue-tsc.js --noEmit` 通过。
3. 根目录 `node node_modules/vite/bin/vite.js build` 通过。
4. 手工：
   - 导入未加密 ed25519 密钥 → 列表出现指纹；
   - 导入带口令密钥：空口令报错、错误口令报错、正确口令成功；
   - 新建 key 认证主机并双击连接成功；
   - 删除被引用密钥被拒绝；删除主机成功；
   - 删除 known_hosts 记录后重连重新写入。
5. 真机密钥登录需要用户提供私钥文件与对应远端 authorized_keys 配置；无可用密钥时以编译/静态验证为准。

## 6. 风险

- Keyring 单条容量有限（Windows 2560 字节），已分块存储；如后续引入便携模式，可在此基础上加主密码加密 vault。
- 右键菜单为自绘实现，需确保滚动/窗口缩放后定位正确（关闭后不残留）。
