# M6 发布手顺：GitHub 仓库、Secret 与首次发布

本手顺覆盖从「本地改动」到「产出一个带自动更新清单的 Release」的完整操作流程。
只涉及仓库与密钥配置，不包含应用功能开发。

---

## 0. 前置条件

| 项 | 要求 |
| --- | --- |
| 仓库可见性 | **Public**（`releases/latest/download/latest.json` 匿名可读） |
| 更新签名私钥 | 本地已生成：`.tauri-keys/mangoshell.key`（已被 `.gitignore` 忽略） |
| 更新签名公钥 | 已写入 `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` |
| 发布 workflow | `.github/workflows/release.yml`（推送 `v*` tag 触发） |

> 私钥一旦丢失，已发布的客户端将无法再通过自动更新升级，必须重新分发包。
> 建议把私钥备份到密码管理器中，并额外保存一份离线副本。

---

## 1. 提交当前改动

```powershell
cd e:\workpace\SourceCode\MangoShell
git add -A
git commit -m "feat: M6 加固发布（CSP/权限收敛、自动更新、多平台 CI、E2E）"
```

提交后确认私钥没有被纳入版本控制：

```powershell
git status --short          # 不应出现 .tauri-keys/
git check-ignore -v .tauri-keys/mangoshell.key
```

---

## 2. 创建 GitHub 仓库

### 方式 A：网页创建（推荐）

1. 打开 <https://github.com/new>
2. 填写：
   - **Repository name**：`MangoShell`
   - **Description**：`自研 SSH 客户端 - Tauri 2 + Vue 3`
   - **Visibility**：`Public`
3. **不要**勾选 `Add a README file` / `Add .gitignore` / `Choose a license`
   （本地已有代码，初始化会产生冲突提交）
4. 点击 **Create repository**

### 方式 B：gh CLI

```powershell
gh auth login                    # 首次使用需登录
gh repo create MangoShell --public --source=. --remote=origin
```

执行后跳过第 3 步的 `git remote add`。

---

## 3. 推送 main 分支

```powershell
# 方式 A 创建仓库时需要先手动添加远程
git remote add origin https://github.com/<你的用户名>/MangoShell.git

git branch -M main
git push -u origin main
```

推送后打开仓库的 **Actions** 页面，确认 `ci` workflow（web / rust / e2e）开始运行。
`e2e` job 会跑 Windows 与 Linux 两个平台，首次运行需要下载 Rust 依赖，耗时较长属正常。

---

## 4. 配置更新签名 Secret

自动更新的安装包必须用私钥签名，CI 需要拿到这把私钥。

### 4.1 读取本地私钥

```powershell
Get-Content .tauri-keys/mangoshell.key -Raw
```

整段 base64 是一行，形如 `dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5...`。
复制的就是这一整行（含结尾换行不影响）。

> 该密钥生成时未设置密码，因此**不需要**配置 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
> 若日后重新生成带密码的密钥，需额外添加同名 `_PASSWORD` Secret，并同步更新 `tauri.conf.json` 里的 `pubkey`。

### 4.2 写入仓库 Secret

1. 打开仓库页面 → **Settings**
2. 左侧 **Secrets and variables** → **Actions**
3. 点击 **New repository secret**
4. 填写：
   - **Name**：`TAURI_SIGNING_PRIVATE_KEY`
   - **Secret**：粘贴 4.1 复制的内容
5. 点击 **Add secret**

### 4.3 用 gh CLI 写入（可选）

```powershell
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo <你的用户名>/MangoShell < .tauri-keys/mangoshell.key
```

验证：

```powershell
gh secret list --repo <你的用户名>/MangoShell
# 应看到 TAURI_SIGNING_PRIVATE_KEY
```

> `GITHUB_TOKEN` 由 Actions 自动注入，不需要手动创建。

---

## 5. 触发首次发布

### 5.1 核对版本号

发布 tag 由 workflow 用 `v__VERSION__` 模板生成，其中的版本号取自 `src-tauri/tauri.conf.json`。
三处版本号必须一致，否则会产生错位的 Release：

| 文件 | 字段 |
| --- | --- |
| `src-tauri/tauri.conf.json` | `version` |
| `src-tauri/Cargo.toml` | `[package] version` |
| `package.json` | `version` |

当前三处均为 `0.1.0`。

### 5.2 打 tag 并推送

```powershell
git tag v0.1.0
git push origin v0.1.0
```

### 5.3 观察构建

打开仓库 **Actions** → `release` workflow，矩阵会并行跑三个平台：

| Job | 产物 |
| --- | --- |
| `publish (windows-x64)` | `*.nsis.zip` + 对应 `.sig` |
| `publish (linux-x64)` | `*.deb`、`*.AppImage` + `.sig` |
| `publish (macos-universal)` | `*.dmg`、`*.app.tar.gz` + `.sig` |

全部成功后，`tauri-action` 会自动创建并发布 Release（非 Draft）。

---

## 6. 验证发布产物

1. 打开仓库 **Releases** 页面，确认存在 `MangoShell v0.1.0`
2. Assets 中必须包含 **`latest.json`**（自动更新清单，缺失说明 `createUpdaterArtifacts` 未生效）
3. 直接访问以下地址，应返回 JSON 而不是 404：

   ```text
   https://github.com/<你的用户名>/MangoShell/releases/latest/download/latest.json
   ```

4. 检查 JSON 中的 `version` 与 `platforms` 键（`windows-x86_64` / `linux-x86_64` / `darwin-*`）齐全

---

## 7. 验证自动更新链路

1. 在任一机器上安装 `0.1.0` 版本并启动
2. 按第 8 步发布 `0.1.1`
3. 重启已安装的 `0.1.0`：
   - 启动后应弹出 toast：`发现新版本 v0.1.1`
   - 点击「安装并重启」后下载并自动重启为 `0.1.1`
4. 无提示时按第 9 节逐项排查

---

## 8. 后续发版流程

```powershell
# 1) 修改三处版本号（见 5.1），例如都改成 0.1.1

# 2) 提交
git add -A
git commit -m "chore: bump version to 0.1.1"

# 3) 推送代码与 tag
git push origin main
git tag v0.1.1
git push origin v0.1.1
```

发版检查清单：

- [ ] 三处版本号一致
- [ ] `git tag` 只存在于新版本，未复用旧 tag
- [ ] Actions 三个平台全部为绿色
- [ ] Release Assets 含 `latest.json` 与各平台 `.sig`
- [ ] 上一版本客户端能成功升级到新版本

---

## 9. 故障排查

| 现象 | 原因 | 处理 |
| --- | --- | --- |
| 构建报签名相关错误 / `signature` 校验失败 | Secret 未配置，或私钥与 `pubkey` 不匹配 | 重新按第 4 节写 Secret；确认 `pubkey` 来自同一对密钥 |
| Release 已生成但无 `latest.json` | `createUpdaterArtifacts` 未生效 | 确认构建使用了 `--config src-tauri/tauri.release.conf.json` |
| 客户端启动后不提示更新 | Release 仍是 Draft / 仓库为 Private / 端点 404 | 发布 Release；改为 Public；用第 6 节地址自测 |
| 客户端提示有新版本但下载失败 | `tauri.conf.json` 的 `version` 未小于远端版本，或安装包签名不匹配 | 核对版本号；确认上传了 `.sig` 文件 |
| 同一版本生成了两个 Release | 本地 tag 与 `tauri.conf.json` 版本号不一致 | 删除错误 Release 与 tag，统一版本号后重推 |
| CI `e2e` 失败在「安装 tauri-driver」 | 依赖编译超时 | 重跑 job；必要时在 workflow 中加 rust-cache 命中检查 |
| CI `e2e` Windows 步骤找不到 msedgedriver | Edge 版本与驱动版本不匹配 | workflow 已回退到同主版本最新驱动，仍失败则手动指定 `MSEDGEDRIVER_PATH` |

---

## 附录：本地等价命令

```powershell
# 仅构建安装包（不发布），用于本地验证签名流程
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content .tauri-keys/mangoshell.key -Raw
pnpm tauri build --config src-tauri/tauri.release.conf.json

# 本地跑 E2E（需先 cargo install tauri-driver --locked）
pnpm tauri build --debug --no-bundle
pnpm e2e
```

> 本地构建若未设置 `endpoints`，应用启动时的更新检查会失败并静默跳过，属预期行为。
