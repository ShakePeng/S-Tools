#!/usr/bin/env bash

set -euo pipefail

identity="${S_TOOLS_RELEASE_SIGNING_IDENTITY:-S-Tools Release Signing}"
keychain="${S_TOOLS_SIGNING_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
certificate="$script_dir/macos-release-signing/s-tools-release-signing.cer"

if [[ ! -f "$certificate" ]]; then
  echo "未找到发布签名公钥证书：$certificate" >&2
  exit 1
fi

if security find-certificate -c "$identity" "$keychain" >/dev/null 2>&1 &&
  security dump-trust-settings 2>/dev/null | grep -Fq "$identity"; then
  echo "发布签名证书“${identity}”已受信任。"
  exit 0
fi

if ! security find-certificate -c "$identity" "$keychain" >/dev/null 2>&1; then
  security import "$certificate" \
    -k "$keychain" \
    -T /usr/bin/codesign
fi

security add-trusted-cert \
  -p codeSign \
  -r trustRoot \
  -k "$keychain" \
  "$certificate"

verification="$(security verify-cert -c "$certificate" -p codeSign -k "$keychain" 2>&1 || true)"
if ! printf '%s\n' "$verification" | grep -q 'No error'; then
  printf '%s\n' "$verification" >&2
  exit 1
fi

echo "已信任发布签名证书“${identity}”。"
