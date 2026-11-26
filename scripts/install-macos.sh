#!/bin/bash
# 安装 MiniPaste 到 /Applications：退出正在运行的 MiniPaste → 替换 → 重新打开。
#
#   bash install-macos.sh            从 GitHub Release 下载最新版安装（README 里的 curl 一行命令走这里）
#   bash install-macos.sh <App路径>  安装本地打包的 App（npm run install:app 走这里）
#
# 本地模式用「移动」而不是「复制」：本机只留一份同 bundle id 的 App，LaunchServices 和登录项不会找错副本。
# 所有发布包都用同一张证书签名（见 sign-macos.sh），所以覆盖安装后辅助功能授权仍然有效。
#
# 注意：会通过 curl | bash 执行，不能依赖脚本所在目录。
set -euo pipefail

REPO=xbzzoo/MiniPaste
DEST=/Applications/MiniPaste.app
BUNDLE_ID=com.minipaste.app

bundle_id() { /usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$1/Contents/Info.plist" 2>/dev/null || true; }

if [ $# -gt 0 ]; then
  SRC="$1"
  [ -d "$SRC" ] || { echo "找不到 ${SRC}，先运行 npm run build:app" >&2; exit 1; }
else
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  echo "下载最新版 MiniPaste"
  # curl 下载的文件没有隔离属性（quarantine），装好可以直接打开；浏览器下载的会被门禁拦下
  curl -fL --progress-bar -o "$tmp/MiniPaste.zip" "https://github.com/$REPO/releases/latest/download/MiniPaste.zip"
  ditto -x -k "$tmp/MiniPaste.zip" "$tmp"   # ditto 保留签名和扩展属性，unzip 不保证
  SRC="$tmp/MiniPaste.app"
fi

[ "$(bundle_id "$SRC")" = "$BUNDLE_ID" ] || { echo "$SRC 不是 MiniPaste" >&2; exit 1; }

# 目标位置已有同名 App 但不是 MiniPaste，不动它
if [ -e "$DEST" ] && [ "$(bundle_id "$DEST")" != "$BUNDLE_ID" ]; then
  echo "$DEST 不是 MiniPaste（bundle id 不符），已停止" >&2
  exit 1
fi

# 退出所有正在运行的 MiniPaste（包括从编译目录启动的）。
# 用信号而不是 osascript quit：后者会弹「终端想要控制 MiniPaste」的自动化授权。
# 菜单里的「退出」本身就是直接结束进程，没有额外的收尾工作，SIGTERM 与之等价。
if pgrep -x mini-paste >/dev/null; then
  echo "退出正在运行的 MiniPaste"
  pkill -x mini-paste || true
  for _ in {1..30}; do pgrep -x mini-paste >/dev/null || break; sleep 0.1; done
  pkill -9 -x mini-paste 2>/dev/null || true
fi

rm -rf "$DEST"
mv "$SRC" "$DEST"
echo "已安装：$DEST"

open "$DEST"
