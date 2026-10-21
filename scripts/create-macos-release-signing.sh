#!/usr/bin/env bash

set -euo pipefail
umask 077

identity="${S_TOOLS_RELEASE_SIGNING_IDENTITY:-S-Tools Release Signing}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
certificate_dir="$repo_root/scripts/macos-release-signing"
public_certificate="$certificate_dir/s-tools-release-signing.cer"
output_dir="${S_TOOLS_RELEASE_SIGNING_DIR:-$repo_root/release/macos-release-signing}"
private_key="$output_dir/private-key.pem"
bundle="$output_dir/identity.p12"
password_file="$output_dir/p12-password.txt"

if [[ -f "$public_certificate" && -f "$bundle" ]]; then
  echo "发布签名证书已存在：$public_certificate"
  echo "私钥和 P12 已存在：$output_dir"
  exit 0
fi

if [[ -f "$public_certificate" || -f "$bundle" || -f "$private_key" ]]; then
  echo "检测到不完整的发布签名证书文件，为避免生成新身份，已停止。" >&2
  echo "请先从备份恢复缺失文件；不要在有部分证书的情况下重复生成。" >&2
  exit 1
fi

mkdir -p "$certificate_dir" "$output_dir"
temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/s-tools-release-signing.XXXXXX")"
trap 'rm -rf "$temporary_dir"' EXIT

temporary_certificate="$temporary_dir/certificate.pem"
temporary_private_key="$temporary_dir/private-key.pem"
temporary_bundle="$temporary_dir/identity.p12"

password="${S_TOOLS_RELEASE_CERT_PASSWORD:-}"
if [[ -z "$password" ]]; then
  password="$(/usr/bin/openssl rand -hex 32)"
fi

/usr/bin/openssl req \
  -x509 \
  -newkey rsa:2048 \
  -sha256 \
  -days 3650 \
  -nodes \
  -keyout "$temporary_private_key" \
  -out "$temporary_certificate" \
  -subj "/CN=$identity" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" \
  -addext "subjectKeyIdentifier=hash" \
  -addext "authorityKeyIdentifier=keyid:always"

/usr/bin/openssl pkcs12 \
  -export \
  -descert \
  -inkey "$temporary_private_key" \
  -in "$temporary_certificate" \
  -name "$identity" \
  -out "$temporary_bundle" \
  -passout "pass:$password"

install -m 644 "$temporary_certificate" "$public_certificate"
install -m 600 "$temporary_private_key" "$private_key"
install -m 600 "$temporary_bundle" "$bundle"
printf '%s\n' "$password" >"$password_file"
chmod 600 "$password_file"

echo "已生成固定发布签名证书“${identity}”。"
echo "公钥证书：$public_certificate"
echo "私钥与 P12：$output_dir"
echo "P12 密码文件：$password_file"
