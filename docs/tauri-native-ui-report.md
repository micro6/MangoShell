# Tauri 原生组件替代评估报告

- 项目：MangoShell（Tauri 2 + Vue 3 + russh）
- 范围：右键菜单及其他适合下沉到 Tauri 原生能力的 UI 组件
- 结论日期：2026-11-19

## 1. 结论摘要

1. Tauri 2 **原生支持右键菜单**，通过核心 `menu` 模块即可在任意位置弹出系统菜单，无需插件。
2. 本项目现有两处自绘右键菜单（主机树、SFTP 文件列表）均可替换，且菜单项都较简单，迁移成本低。
3. 除右键菜单外，确认框、文件选择、拖放、托盘、窗口状态、通知、全局快捷键、开机自启、自动更新都有成熟的原生方案。
4. 标签栏、状态栏、主机树、表单、toast 等强交互组件**不建议**原生替代。

## 2. 原生右键菜单

### 2.1 能力说明

Tauri 2 的核心 `@tauri-apps/api/menu` 提供 `Menu`、`MenuItem`、`Submenu`、`PredefinedMenuItem`、`CheckMenuItem`、`IconMenuItem`，可在指定坐标弹出系统菜单。

- Windows：Win32 菜单，系统主题。
- macOS：NSMenu。
- Linux：GTK 菜单，依赖系统 GTK 运行库。

### 2.2 权限配置

需要在 capability 中启用菜单权限：

```json
{
  "windows": ["main", "window-*"],
  "permissions": ["core:default", "core:menu:default"]
}
```

### 2.3 示例

```ts
import { Menu, MenuItem, PredefinedMenuItem } from "@tauri-apps/api/menu";
import { LogicalPosition } from "@tauri-apps/api/dpi";

const menu = await Menu.new({
  items: [
    await MenuItem.new({ id: "connect", text: "连接", action: () => connect() }),
    await MenuItem.new({ id: "edit", text: "编辑", action: () => edit() }),
    await PredefinedMenuItem.new({ item: "Separator" }),
    await MenuItem.new({ id: "delete", text: "删除", action: () => remove() }),
  ],
});

// 不传坐标时默认弹在鼠标位置；传坐标时注意使用逻辑像素
await menu.popup(new LogicalPosition(event.clientX, event.clientY));
```

对应 `contextmenu` 处理必须先 `preventDefault`，避免 WebView 默认菜单同时出现。

### 2.4 边界与限制

- 外观、圆角、间距完全由系统决定，无法与项目自绘风格完全一致。
- 不能嵌入任意 HTML，不适合搜索框、分组标题、动态表单等富内容。
- 图标、勾选、子菜单、快捷键、禁用态支持完整；分隔线使用 `PredefinedMenuItem`。
- 每次弹出建议创建新菜单或复用后及时释放，避免长期持有大量菜单对象。
- 在高 DPI 屏幕上，坐标需使用逻辑像素；不确定缩放时可直接省略坐标，交给系统定位。

### 2.5 本项目适配点

| 位置 | 现有实现 | 现有菜单项 | 迁移建议 |
| --- | --- | --- | --- |
| 主机树 | `HostTree.vue` 中 `Teleport` + 自绘 `.menu` | 连接、编辑、删除 | 推荐迁移，菜单项固定且含危险项 |
| SFTP 文件列表 | `FilePane.vue` 中绝对定位 `.menu` | 下载/上传、重命名、权限、删除 | 推荐迁移，需按 `isDir`、`isRemote` 动态增删项 |
| 终端区域 | xterm 默认右键行为 | 无 | 可后续增加复制/粘贴/清屏原生菜单 |

迁移后可以删除自绘菜单的遮罩层、定位计算和点击外部关闭逻辑。

## 3. 其它可原生替代的组件

| 现有 Web 实现 | 原生方案 | 收益 | 风险/代价 |
| --- | --- | --- | --- |
| 删除确认 `AlertDialog` | `plugin-dialog` 的 `confirm` / `ask` | 系统模态、阻塞语义清晰 | 样式与自绘不一致 |
| 密钥导入、日志保存的文件选择 | `plugin-dialog` 的 `open` / `save` | 已在使用，无额外成本 | 无 |
| 拖拽本地文件到 SFTP | `onDragDropEvent`（`tauri://drag-drop` 等） | 能拿到真实本地路径，HTML5 drop 拿不到 | 需确认窗口未禁用拖放 |
| 最小化/最大化/关闭/全屏 | `@tauri-apps/api/window` | 行为与系统一致 | 若保留自绘标题栏，仍需自行处理拖拽区域 |
| 深色模式跟随系统 | `theme()` + `onThemeChanged` | 无需手动判断系统主题 | 需同步 CSS 变量 |
| 托盘常驻与托盘菜单 | Rust `TrayIconBuilder` | 后台保活、快速连接入口 | Linux 需要 appindicator 依赖 |
| 应用菜单栏、关于、退出 | `Menu` + `PredefinedMenuItem` | 快捷键与系统习惯一致 | 与终端快捷键可能冲突，需要白名单 |
| 操作完成/失败通知 | `plugin-notification` | 窗口在后台也能提示 | 不适合替代轻量 toast |
| 全局快捷键（呼出窗口） | `plugin-global-shortcut` | 跨应用可用 | 需处理快捷键冲突与注册失败 |
| 单实例运行 | `plugin-single-instance` | 避免多实例抢数据库 | 多窗口模式需明确“第二实例只聚焦已有窗口” |
| 窗口位置/大小记忆 | `plugin-window-state` | 重启恢复布局 | 多窗口需要按 label 区分策略 |
| 开机自启 | `plugin-autostart` | 常驻场景需要 | 需在设置中提供开关 |
| 自动更新 | `plugin-updater` | 正式分发必需 | 需要签名密钥与更新端点 |

## 4. 不建议原生替代

| 组件 | 原因 |
| --- | --- |
| 标签栏、状态栏 | 需要拖拽排序、脉冲状态、主题色等自定义表现 |
| 主机树、文件列表 | 需要搜索、滚动虚拟化、选中态与图标组合 |
| 主机表单 | 需要字段级校验、即时提示与统一视觉 |
| toast | 原生通知是系统级提醒，不适合高频轻量反馈 |
| 终端搜索条 | 与 xterm 强耦合，必须保留 Web 层实现 |

## 5. 实施优先级建议

| 优先级 | 内容 | 说明 |
| --- | --- | --- |
| P0 | 主机树、SFTP 文件列表右键菜单 | 现状自绘代码可完全删除，收益直接 |
| P0 | 删除确认改用原生 `confirm` | 与右键菜单同一批改动，减少自绘弹窗 |
| P1 | 本地文件拖放到 SFTP、窗口状态记忆 | 明显改善日常使用体验 |
| P1 | 托盘常驻 + 托盘菜单 | 多窗口/后台场景的入口 |
| P2 | 全局快捷键、开机自启、系统通知 | 依赖产品形态，按需开启 |
| P2 | 自动更新 | 正式发布阶段再接入，需要签名与更新服务 |

## 6. 迁移注意事项

1. **能力声明**：菜单、对话框、通知等都需要在 `src-tauri/capabilities/*.json` 中显式授权；多窗口项目要把新增窗口 label 加入 `windows` 匹配。
2. **多窗口**：原生菜单与窗口绑定，右键菜单应按当前窗口创建；托盘菜单属于应用级，需避免重复创建。
3. **事件清理**：菜单项回调若引用组件状态，组件卸载后应释放菜单对象，避免持有失效引用。
4. **快捷键冲突**：应用菜单的快捷键会先于 WebView 生效，`Ctrl+F`、`Ctrl+W` 等需要评估是否交给系统菜单。
5. **平台差异**：Linux 依赖 GTK/appindicator；macOS 菜单在屏幕顶部而非窗口内；Windows 需注意高 DPI 坐标换算。
6. **回退策略**：原生能力初始化失败（如托盘不可用）时，应保留 Web UI 入口，避免功能完全不可达。

## 7. 参考

- Tauri 2 Menu API：`@tauri-apps/api/menu`
- Tauri 2 Dialog Plugin：`@tauri-apps/plugin-dialog`
- Tauri 2 Window API：`@tauri-apps/api/window`
- Tauri 2 Webview 拖放事件：`getCurrentWebview().onDragDropEvent()`
- 官方插件：`notification`、`global-shortcut`、`single-instance`、`window-state`、`autostart`、`updater`
