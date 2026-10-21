#!/usr/bin/env bash

set -euo pipefail
umask 077

identity="${S_TOOLS_SIGNING_IDENTITY:-S-Tools Local Dev}"
keychain="${S_TOOLS_SIGNING_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"

has_identity() {
  security find-identity -v -p codesigning "$keychain" 2>/dev/null |
    grep -Fq "\"$identity\""
}

if [[ "${1:-}" == "--remove" ]]; then
  if has_identity; then
    security delete-identity -c "$identity" -t "$keychain"
    echo "已删除本地开发签名身份“${identity}”。"
  elif security find-certificate -c "$identity" "$keychain" >/dev/null 2>&1; then
    security delete-certificate -c "$identity" -t "$keychain"
    echo "已删除不完整的本地开发签名证书“${identity}”。"
  else
    echo "本地开发签名身份“${identity}”不存在，无需删除。"
  fi
  exit 0
fi

if has_identity; then
  echo "本地开发签名身份“${identity}”已存在。"
  exit 0
fi

temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/s-tools-dev-signing.XXXXXX")"
trap 'rm -rf "$temporary_dir"' EXIT

certificate="$temporary_dir/certificate.pem"
private_key="$temporary_dir/private-key.pem"
bundle="$temporary_dir/identity.p12"
bundle_password="$(/usr/bin/openssl rand -hex 24)"

if security find-certificate -c "$identity" "$keychain" >/dev/null 2>&1; then
  security find-certificate -c "$identity" -p "$keychain" >"$certificate"
  security add-trusted-cert \
    -p codeSign \
    -r trustRoot \
    -k "$keychain" \
    "$certificate"
  if has_identity; then
    echo "已修复并信任本地开发签名身份“${identity}”。"
    exit 0
  fi
  echo "证书“${identity}”已存在，但缺少可用的私钥或信任设置。" >&2
  echo "请先执行 yarn tauri:dev:setup:remove，再重新初始化。" >&2
  exit 1
fi

/usr/bin/openssl req \
  -x509 \
  -newkey rsa:2048 \
  -sha256 \
  -days 3650 \
  -nodes \
  -keyout "$private_key" \
  -out "$certificate" \
  -subj "/CN=$identity" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning"

/usr/bin/openssl pkcs12 \
  -export \
  -descert \
  -inkey "$private_key" \
  -in "$certificate" \
  -name "$identity" \
  -out "$bundle" \
  -passout "pass:$bundle_password"

security import "$bundle" \
  -k "$keychain" \
  -P "$bundle_password" \
  -T /usr/bin/codesign \
  -T /usr/bin/security

security add-trusted-cert \
  -p codeSign \
  -r trustRoot \
  -k "$keychain" \
  "$certificate"

if ! has_identity; then
  echo "证书已导入，但系统仍未将其识别为可用的代码签名身份。" >&2
  exit 1
fi

echo "已创建并信任本地开发签名身份“${identity}”。"
echo "后续执行 yarn tauri:dev 会自动签名，无需重新打包。"
