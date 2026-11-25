#!/bin/bash
# 用本机自签名证书给 MiniPaste.app 重新签名，让「辅助功能」授权在重新打包后依然有效。
#
# 为什么需要：tauri 默认做 ad-hoc 签名，designated requirement 是 cdhash，每次编译都会变。
# 系统按 requirement 认 App，于是新包被当成另一个 App，之前给的授权对它无效——
# 设置里的开关还开着，但每次粘贴都弹权限申请。
# 换成固定证书签名后，requirement 变成「bundle id + 证书」，重新编译也不变，授权一次长期有效。
#
# 证书只在本机生成一次，放在独立钥匙串 ~/Library/Keychains/minipaste-signing.keychain-db，
# 不碰登录钥匙串；签名时把它临时加进钥匙串搜索列表（codesign 只在搜索列表里找身份），签完立即还原。
#
# 用法：bash scripts/sign-macos.sh [MiniPaste.app 路径]
set -euo pipefail

cd "$(dirname "$0")/.."
APP="${1:-src-tauri/target/release/bundle/macos/MiniPaste.app}"
KEYCHAIN="$HOME/Library/Keychains/minipaste-signing.keychain-db"
# 只用来保护这张本机自签证书（它除了给本机的 MiniPaste 签名没有任何权限），不是机密
PASSWORD="minipaste-local"
IDENTITY="MiniPaste Local Signing"
# 必须用系统自带的 LibreSSL：Homebrew 的 OpenSSL 3+ 导出的 p12 用了新 MAC 算法，macOS 导入不了
OPENSSL=/usr/bin/openssl

[ -d "$APP" ] || { echo "找不到 $APP，先运行 npm run tauri build -- --bundles app" >&2; exit 1; }

if [ ! -f "$KEYCHAIN" ]; then
  echo "首次运行：生成本机自签名代码签名证书 → $KEYCHAIN"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  cat > "$tmp/openssl.cnf" <<'EOF'
[req]
distinguished_name = dn
[dn]
[v3]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF
  "$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -keyout "$tmp/key.pem" -out "$tmp/cert.pem" \
    -subj "/CN=$IDENTITY" -config "$tmp/openssl.cnf" -extensions v3 2>/dev/null
  "$OPENSSL" pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
    -name "$IDENTITY" -out "$tmp/identity.p12" -passout "pass:$PASSWORD"

  security create-keychain -p "$PASSWORD" "$KEYCHAIN"
  security set-keychain-settings "$KEYCHAIN"   # 不自动上锁
  security import "$tmp/identity.p12" -k "$KEYCHAIN" -P "$PASSWORD" -T /usr/bin/codesign >/dev/null
  # 允许 codesign 直接用私钥，不弹「codesign 想要访问钥匙串」
  security set-key-partition-list -S apple-tool:,apple: -s -k "$PASSWORD" "$KEYCHAIN" >/dev/null
  rm -rf "$tmp"
  trap - EXIT
fi

security unlock-keychain -p "$PASSWORD" "$KEYCHAIN"

# 记下原来的搜索列表，无论成功失败都还原
original=()
while IFS= read -r line; do
  line="${line#"${line%%[![:space:]]*}"}"   # 去掉前导空白
  line="${line#\"}"; line="${line%\"}"       # 去掉引号
  [ -n "$line" ] && original+=("$line")
done < <(security list-keychains -d user)
restore() { security list-keychains -d user -s "${original[@]}"; }
trap restore EXIT

security list-keychains -d user -s "${original[@]}" "$KEYCHAIN"
codesign --force --timestamp=none --sign "$IDENTITY" "$APP"
restore
trap - EXIT

echo "已签名：$(codesign -d -r- "$APP" 2>&1 | tail -1)"
