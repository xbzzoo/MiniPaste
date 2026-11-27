# MiniPaste

macOS 菜单栏剪贴板工具：自动记录复制过的内容，按 `⇧⌘V` 从屏幕底部弹出面板，选中即粘贴回原来的 App。

## 功能

- 记录文本、富文本、链接、邮箱、颜色、路径、图片、文件，可按类型筛选
- 弹出面板直接打字即可搜索全部历史（多个词用空格隔开，图片不参与）
- 底部面板，选中后自动粘贴（带提示音）
- 默认显示最近 24 小时，可切换到一周或全部
- 查看占用空间，按时间清理历史
- 修改快捷键、开机自动启动、隐藏菜单栏图标

## 安装

支持 Apple 芯片和 Intel，macOS 11 及以上。终端运行：

```bash
curl -fsSL https://raw.githubusercontent.com/xbzzoo/MiniPaste/main/scripts/install-macos.sh | bash
```

装到 `/Applications` 并自动打开；再运行一次就是更新到最新版，历史、设置和辅助功能授权都保留。

也可以在 [Releases](https://github.com/xbzzoo/MiniPaste/releases) 下载 `MiniPaste.zip` 手动安装。App 未经 Apple 公证，这样下载的第一次打开会被拦下，到「系统设置 → 隐私与安全性」点「仍要打开」即可。

从源码安装：`npm install && npm run install:app`。

## 使用

| 按键 | 作用 |
| --- | --- |
| `⇧⌘V` | 打开 / 关闭面板 |
| `←` `→` | 选择 |
| `↵` / 双击 | 粘贴 |
| 直接打字 | 搜索 |
| `⌫` | 删除（搜索时用 `⌘⌫`） |
| `tab` | 切换类型 |
| `esc` | 清空搜索 / 关闭 |

其他设置在菜单栏图标或面板右上角「⋯」的菜单里。

自动粘贴需要辅助功能权限：第一次粘贴时按提示到「系统设置 → 隐私与安全性 → 辅助功能」打开 MiniPaste。未授权时内容仍会复制到剪贴板，手动 `⌘V` 即可。

## 数据

全部保存在本机 `~/Library/Application Support/com.minipaste.app/`：

- `history.db`：历史记录（SQLite）
- `images/`：图片及缩略图
- `config.json`：设置（快捷键、显示范围、最多保留条数 `maxItems` 等）

默认最多保留 10000 条，超出删除最久未用的；密码管理器标记为隐藏的内容不会记录。

## 开发

- `npm run tauri dev`：开发运行
- `npm run build:app`：只打包不安装，产物在 `src-tauri/target/release/bundle/macos`；会用本机自签名证书签名（`scripts/sign-macos.sh`），重新打包后辅助功能授权不会失效
- `npm run release`：发布新版本到 GitHub Release（先改 `tauri.conf.json` 里的版本号）。必须用同一张签名证书，否则用户更新后要重新授权辅助功能
- 测试：`cd src-tauri && cargo test`
- 图标源文件在 `assets/`

```text
src-tauri/src/
  clipboard.rs  监听剪贴板、归类入库、粘贴写回
  db.rs         SQLite 存储：分页、计数、清理
  detect.rs     文本类型识别
  panel.rs      底部面板
  menu.rs       菜单
  shortcut.rs   全局快捷键与修改窗口
  autostart.rs  开机自动启动
  native.rs     提示音、确认弹窗
  paste.rs      模拟 ⌘V
  config.rs     配置读写
src/            前端（React）
```

## 许可证

[MIT](LICENSE)
