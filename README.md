# MiniPaste

macOS 菜单栏剪贴板工具：自动记录复制过的内容，按 `⇧⌘V` 从屏幕底部弹出面板，选中即粘贴回原来的 App。

## 功能

- 记录文本、富文本、链接、邮箱、颜色、路径、图片、文件，可按类型筛选
- 底部面板，选中后自动粘贴（带提示音）
- 默认显示最近 24 小时，可切换到一周或全部
- 查看占用空间，按时间清理历史
- 修改快捷键、开机自动启动、隐藏菜单栏图标

## 运行

```bash
npm install
npm run tauri dev     # 开发
npm run build:app     # 打包，产物在 src-tauri/target/release/bundle/macos
```

## 使用

| 按键 | 作用 |
| --- | --- |
| `⇧⌘V` | 打开 / 关闭面板 |
| `←` `→` | 选择 |
| `↵` / 双击 | 粘贴 |
| `⌫` | 删除 |
| `tab` | 切换类型 |
| `esc` | 关闭 |

其他设置在菜单栏图标或面板右上角「⋯」的菜单里。

自动粘贴需要辅助功能权限：第一次粘贴时按提示到「系统设置 → 隐私与安全性 → 辅助功能」打开 MiniPaste。未授权时内容仍会复制到剪贴板，手动 `⌘V` 即可。

## 数据

全部保存在本机 `~/Library/Application Support/com.minipaste.app/`：

- `history.db`：历史记录（SQLite）
- `images/`：图片及缩略图
- `config.json`：设置（快捷键、显示范围、最多保留条数 `maxItems` 等）

默认最多保留 10000 条，超出删除最久未用的；密码管理器标记为隐藏的内容不会记录。

## 开发

- `npm run build:app` 会用本机自签名证书签名（`scripts/sign-macos.sh`），重新打包后辅助功能授权不会失效
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
