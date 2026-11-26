#!/bin/bash
# 打通用包（Apple 芯片 + Intel）→ 签名 → 压缩成 MiniPaste.zip → 打 tag 并发布 GitHub Release。
#
# 必须在有签名证书的这台 Mac 上跑（证书在本机钥匙串，见 sign-macos.sh）。每个版本都用同一张证书签名，
# 用户覆盖安装后辅助功能授权才不会失效——换证书（比如改到 CI 里签）会让所有用户重新授权。
#
# 版本号取 src-tauri/tauri.conf.json 的 version，发布前先改它（package.json、Cargo.toml 保持一致）。
# 用法：npm run release [-- 发布说明.md]（不给说明文件就按提交自动生成）
set -euo pipefail

NOTES=""
[ $# -gt 0 ] && NOTES="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
cd "$(dirname "$0")/.."

VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")
TAG="v$VERSION"

[ -z "$(git status --porcelain)" ] || { echo "工作区有未提交的改动，先提交" >&2; exit 1; }
git fetch -q origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse '@{u}')" ] || { echo "本地和远端不一致，先推送或拉取" >&2; exit 1; }
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null || gh release view "$TAG" >/dev/null 2>&1; then
  echo "$TAG 已发布过，先改 tauri.conf.json 里的版本号" >&2
  exit 1
fi

npx tauri build --bundles app --target universal-apple-darwin
OUT=src-tauri/target/universal-apple-darwin/release/bundle
bash scripts/sign-macos.sh "$OUT/macos/MiniPaste.app"

# ditto 保留签名，Finder 的「压缩」也是这个格式。附件名固定为 MiniPaste.zip：
# install-macos.sh 从 releases/latest/download/MiniPaste.zip 下载
ZIP="$OUT/MiniPaste.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$OUT/macos/MiniPaste.app" "$ZIP"

git tag "$TAG"
git push -q origin "$TAG"
if [ -n "$NOTES" ]; then
  gh release create "$TAG" "$ZIP" --title "MiniPaste $VERSION" --notes-file "$NOTES"
else
  gh release create "$TAG" "$ZIP" --title "MiniPaste $VERSION" --generate-notes
fi
