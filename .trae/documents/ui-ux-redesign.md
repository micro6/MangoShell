# MangoShell 轻量视觉优化计划

## Context

放弃 VS Code 式活动栏重构，保留现有布局：顶部标签栏、左侧主机树、工作区工具栏、右侧内容、底部状态栏。

本次只做视觉与交互细节优化，解决：

- 字号、按钮高度、圆角、间距混用，缺少统一尺度。
- 主机树、状态栏、工作区工具栏信息层级弱，视觉噪声多。
- 按钮缺少统一悬停/焦点/禁用态，图标按钮缺少 `aria-label`。
- 字体依赖 Google Fonts，离线桌面环境不可靠。
- `index.html` 仍是默认标题与 Vite 图标。
- 暗色用户首屏存在白闪。

不改动任何业务逻辑、数据流、Rust 后端和现有弹窗结构。

## 视觉规范

- 字号：11 辅助 / 12 正文 / 13 标题；行高 18/20/18。
- 尺寸：标签栏 34、工作区工具栏 34、状态栏 24、列表行 32（主机行 44，双行信息）。
- 圆角：控件 6、菜单/浮层 8；面板保持直角。
- 间距：统一 4/8/12/16，卡片/面板内边距 12。
- 字体：移除 Google Fonts，改用 `system-ui / Microsoft YaHei UI / PingFang SC`；终端继续 JetBrains Mono 回退栈。
- 颜色：沿用现有 token，新增 `--hover`、`--active`、`--tab-active`、`--tab-inactive` 四个语义变量，亮暗各一套。
- 交互态：悬停背景使用 `--hover`，选中使用 `--active`；焦点统一 2px ring，保持现有 `focus-visible` 策略。

## 实施阶段

### 阶段 1：基础样式与首屏

- `index.html`：标题改为 MangoShell、`lang="zh-CN"`、替换默认 favicon、加入主题内联脚本避免白闪。
- `src/styles/index.css`：删除字体 `@import`，补充新语义 token、统一滚动条与选中色。
- `src/styles/abstracts/_variables.scss`：补齐尺寸与圆角常量。
- `src/main.ts`：主题初始化提前到挂载前，并同步 `color-scheme`。

可验证点：亮/暗刷新无白闪、字体回退正常、标题正确。

### 阶段 2：框架区域

- `TabBar.vue`：高度 34，标签紧凑化；品牌文字缩小并降低对比；连接状态点统一为 6px；关闭按钮仅悬停/激活时显示；标签可横向滚动。
- `workspace-toolbar`（`App.vue` 内）：改为「面包屑 + 状态 chip + 操作组」三段式，面包屑显示别名与 `user@host`；操作按钮统一为图标 + 文字，窄屏只留图标。
- `StatusBar.vue`：左侧当前连接状态与主机，右侧版本号；弱化边框与背景，字号 11。

可验证点：三种标签（终端/SFTP/转发）下工具栏信息正确，窄窗口不溢出。

### 阶段 3：主机树

- `HostTree.vue`：搜索框与新建/导入按钮对齐到同一行；主机行改为双行（别名 + `user@host:port`），右侧显示连接状态点（connected 绿、connecting/authenticating 琥珀脉冲、failed 红）。
- 空态：图标 + 说明 + “新建主机”按钮。
- 底部密钥/已知主机按钮改为次级样式，悬停才突出。
- 右键菜单视觉与全局菜单圆角、间距一致。

可验证点：连接状态随标签变化实时更新；空列表有引导；搜索无结果有提示。

### 阶段 4：内容区一致性

- `TerminalPane.vue`：搜索条、录制按钮、重连状态条统一高度与间距；状态条圆角、阴影与全局浮层一致。
- `SftpPane.vue` / `ForwardPane.vue`：工具栏、列表行高、空态、错误态统一；表头与行字号统一 12。
- `KeyManager.vue` / `KnownHosts.vue` / `HostEditor.vue` / `PermissionDialog.vue`：统一标题区、按钮尺寸（`h-8` 表单、`h-7` 紧凑）、输入框间距；列表空态统一。

可验证点：各弹窗在亮/暗下间距一致，按钮高度无跳变。

### 阶段 5：交互细节收口

- 所有图标按钮补 `aria-label` 与 `title`。
- 删除/断开等危险操作按钮统一 `destructive` 样式。
- 统一悬停、按下、禁用、加载态；按钮加载时保留宽度避免抖动。
- 检查焦点顺序与键盘可达性（Tab/Enter/Esc）。

## 主要文件

- `index.html`、`src/main.ts`
- `src/styles/index.css`、`src/styles/abstracts/_variables.scss`
- `src/App.vue`、`src/components/term/TabBar.vue`、`src/components/common/StatusBar.vue`
- `src/components/host/HostTree.vue`
- `src/components/term/TerminalPane.vue`
- `src/components/sftp/SftpPane.vue`、`src/components/forward/ForwardPane.vue`
- `src/components/host/KeyManager.vue`、`KnownHosts.vue`、`HostEditor.vue`、`PermissionDialog.vue`

## 验证

- `pnpm typecheck`、`pnpm exec vp check`、`pnpm build`
- `pnpm exec vp test src/core/term/manager.test.ts --run`（6 个重连测试）
- 手动：亮/暗切换、强制暗色刷新、侧栏折叠、主机连接状态点、空态、终端搜索/录制/重连、SFTP 与转发、各弹窗键盘操作。

## 风险与回滚

- 纯样式与模板结构调整，不改 store、IPC 与 Rust；如出现布局问题可按文件单独回退。
- 主机行改为双行会增加行高，需要确认长别名省略号处理。
- 终端搜索条与状态条改动涉及 xterm 容器尺寸，需确认 fit 不受影响。
- 不改动现有功能入口与弹窗结构，避免影响已验收的密码保存、自动重连与日志录制。
