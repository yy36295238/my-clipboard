# AI Clipboard

一款 macOS 剪贴板历史管理工具，基于 Tauri 2 + React 构建。常驻菜单栏，全局快捷键唤起浮动面板，自动记录复制过的文本和图片，支持智能分类、搜索、收藏和一键回贴。

## 功能特性

- **剪贴板历史**：后台自动监听系统剪贴板（500ms 轮询），文本和图片都会被记录；重复复制相同内容只更新时间，不产生重复条目
- **全局快捷键**：`⌘ + ⇧ + V` 随时唤起/隐藏面板，面板为无边框置顶浮窗（NSPanel），不打断当前应用的焦点
- **一键粘贴**：点击任意条目即写入剪贴板、自动隐藏面板并把 `⌘V` 发送回原应用，无需手动切换窗口
- **智能内容识别**：自动将内容归类为 文本 / JSON / 链接 / 代码 / Markdown / 邮箱 / 手机号 / 图片，按标签页分类浏览，代码片段带语法高亮（highlight.js）
- **搜索与筛选**：关键词搜索，支持按日期筛选（今天 / 昨天 / 前天 / 自定义区间）
- **收藏与片段**：收藏常用内容并设置名称和分组，沉淀为可复用的代码片段 / 文本模板
- **置顶**：重要条目可固定在列表顶部
- **条目管理**：支持编辑、删除单条记录或清空全部历史；图片自动去重（基于内容哈希），删除时同步清理对应图片文件
- **纯菜单栏应用**：通过托盘图标常驻，不占用 Dock（`LSUIElement`）

<img width="1902" height="1328" alt="image" src="https://github.com/user-attachments/assets/47f394c2-6a65-462a-8911-635b479d4d1d" />

## 下载安装

1. 前往 [Releases 页面](https://github.com/yy36295238/my-clipboard/releases/latest) 下载最新的 `.dmg` 安装包（当前仅提供 Apple Silicon 版本）
2. 打开 `.dmg`，把 **AI Clipboard** 拖入 `应用程序` 文件夹
3. 首次打开时 macOS 会提示"无法验证开发者"（应用未做 Apple 签名），按以下任一方式放行：
   - 在终端执行（推荐，一步到位）：
     ```bash
     sudo xattr -cr "/Applications/AI Clipboard.app"
     ```
   - 或在 `应用程序` 中**右键点击应用 → 打开 → 再次点击"打开"**
   - 或前往 **系统设置 → 隐私与安全性**，在底部点击"仍要打开"
4. 启动后应用常驻菜单栏（不在 Dock 显示），按 `⌘ + ⇧ + V` 唤起面板
5. 首次使用"一键粘贴"时，按系统提示在 **系统设置 → 隐私与安全性 → 辅助功能** 中勾选授权本应用

## 数据存储

所有数据仅保存在本地，不会上传到任何服务器：

```
~/Library/Application Support/ai-clipboard/
├── clipboard.db   # SQLite 数据库（文本内容、元数据）
└── images/        # 剪贴板图片（PNG 文件）
```

## 技术栈

| 层 | 技术 |
| --- | --- |
| 前端 | React 19 + TypeScript + Vite |
| 桌面框架 | Tauri 2（Rust） |
| 存储 | rusqlite（内置 SQLite） |
| 剪贴板读写 | arboard |
| 系统集成 | tauri-nspanel（浮动面板）、core-graphics（模拟按键）、tray-icon、global-shortcut |

## 开发与构建

### 环境要求

- macOS（依赖 macOS 私有 API，仅支持 macOS）
- Node.js 18+
- Rust 1.77.2+

### 本地开发

```bash
npm install
./dev.sh        # 等价于 npm run tauri dev
```

### 打包

```bash
./build.sh      # 等价于 npm run tauri build
```

产物在 `src-tauri/target/release/bundle/` 下，包含 `.app` 和 `.dmg`。

### 权限说明

首次使用"一键粘贴"功能时，需要在 **系统设置 → 隐私与安全性 → 辅助功能** 中授权本应用（模拟 `⌘V` 按键需要辅助功能权限）。

## 项目结构

```
├── src/                    # React 前端（面板 UI、标签页、搜索、片段管理）
├── src-tauri/
│   └── src/
│       ├── lib.rs          # 应用入口：托盘、全局快捷键、NSPanel 窗口
│       ├── monitor.rs      # 剪贴板监听、内容类型识别、图片去重
│       ├── commands.rs     # 前端调用的 Tauri 命令（查询、粘贴、收藏等）
│       └── db.rs           # SQLite 持久化
├── dev.sh                  # 开发启动脚本
└── build.sh                # 打包脚本
```

## License

个人项目，仅供学习参考。
