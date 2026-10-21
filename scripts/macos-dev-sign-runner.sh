#!/usr/bin/env bash

set -euo pipefail

identity="${S_TOOLS_SIGNING_IDENTITY:-S-Tools Local Dev}"
binary="${1:?缺少 Cargo 调试二进制路径}"
keychain="${S_TOOLS_SIGNING_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
dev_app_path="${S_TOOLS_DEV_APP_PATH:-$HOME/Applications/S-Tools Dev.app}"

if [[ ! -x "$binary" ]]; then
  echo "无法签名调试二进制：$binary" >&2
  exit 1
fi

if ! security find-identity -v -p codesigning "$keychain" 2>/dev/null | grep -Fq "\"$identity\""; then
  echo "未找到本地代码签名身份“$identity”。" >&2
  echo "请先执行：yarn tauri:dev:setup" >&2
  exit 1
fi

bundle_path="$dev_app_path"
bundle_contents="$bundle_path/Contents"
bundle_executable="$bundle_contents/MacOS/s-tools"

mkdir -p "$bundle_contents/MacOS" "$bundle_contents/Resources"
cp "$script_dir/macos-dev-app/Info.plist" "$bundle_contents/Info.plist"
cp "$repo_root/src-tauri/icons/icon.icns" "$bundle_contents/Resources/icon.icns"
install -m 755 "$binary" "$bundle_executable"

codesign --force --sign "$identity" --timestamp=none "$bundle_path"
codesign --verify --strict "$bundle_path"

shift
exec "$bundle_executable" "$@"
