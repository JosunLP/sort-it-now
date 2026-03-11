#!/usr/bin/env bash
set -euo pipefail

APP_NAME="sort_it_now"
DEFAULT_INSTALL_DIR="/usr/local/bin"
INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
OWNER="${SORT_IT_NOW_GITHUB_OWNER:-JosunLP}"
REPO="${SORT_IT_NOW_GITHUB_REPO:-sort-it-now}"
REQUESTED_VERSION="${SORT_IT_NOW_VERSION:-latest}"
SCRIPT_SOURCE="${BASH_SOURCE[0]:-}"
SCRIPT_DIR="$PWD"

if [[ -n "$SCRIPT_SOURCE" && -e "$SCRIPT_SOURCE" ]]; then
  SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_SOURCE")" && pwd)"
fi

install_local_binary() {
  local binary_path="$1"

  if [[ ! -x "$binary_path" ]]; then
    echo "ℹ️ Setting execute permissions for $binary_path"
    chmod +x "$binary_path"
  fi

  if [[ ! -d "$INSTALL_DIR" ]]; then
    echo "ℹ️ Creating installation directory $INSTALL_DIR"
    mkdir -p "$INSTALL_DIR"
  fi

  if [[ ! -w "$INSTALL_DIR" ]]; then
    echo "⚠️ Write permissions missing in $INSTALL_DIR. Try using 'sudo'." >&2
    exit 1
  fi

  install -m 755 "$binary_path" "$INSTALL_DIR/$APP_NAME"

  echo "✅ $APP_NAME was successfully installed to $INSTALL_DIR."
  echo "ℹ️ Start the service with: $APP_NAME"
}

detect_target_suffix() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os/$arch" in
    Linux/x86_64|Linux/amd64)
      printf '%s\n' "linux-x86_64"
      ;;
    Darwin/arm64|Darwin/aarch64)
      printf '%s\n' "macos-arm64"
      ;;
    Darwin/x86_64)
      printf '%s\n' "macos-x86_64"
      ;;
    *)
      echo "❌ Unsupported platform for one-command install: $os/$arch" >&2
      exit 1
      ;;
  esac
}

parse_release_asset_urls() {
  local suffix="$1"
  python - "$suffix" <<'PY'
import json
import sys

suffix = sys.argv[1]
release = json.load(sys.stdin)
archive = None
checksum = None

for asset in release.get("assets", []):
    name = asset.get("name", "")
    url = asset.get("browser_download_url", "")
    if name.endswith(f"{suffix}.tar.gz"):
        archive = url
    if name.endswith(f"{suffix}.tar.gz.sha256"):
        checksum = url

if not archive:
    raise SystemExit(1)

print(archive)
print(checksum or "")
PY
}

download_and_install_latest_release() {
  local suffix api_url auth_header tmp_dir archive_path checksum_path release_json archive_url checksum_url bundle_dir expected_checksum computed_checksum
  suffix="$(detect_target_suffix)"
  if [[ "$REQUESTED_VERSION" == "latest" ]]; then
    api_url="https://api.github.com/repos/$OWNER/$REPO/releases/latest"
  else
    api_url="https://api.github.com/repos/$OWNER/$REPO/releases/tags/$REQUESTED_VERSION"
  fi

  auth_header=()
  if [[ -n "${SORT_IT_NOW_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}" ]]; then
    auth_header=(-H "Authorization: Bearer ${SORT_IT_NOW_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}")
  fi

  release_json="$(curl -fsSL "${auth_header[@]}" -H "Accept: application/vnd.github+json" "$api_url")"
  mapfile -t asset_urls < <(printf '%s' "$release_json" | parse_release_asset_urls "$suffix")
  archive_url="${asset_urls[0]:-}"
  checksum_url="${asset_urls[1]:-}"

  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT
  archive_path="$tmp_dir/release.tar.gz"
  checksum_path="$tmp_dir/release.tar.gz.sha256"

  echo "⬇️ Downloading sort-it-now release for $suffix..."
  curl -fsSL "${auth_header[@]}" -o "$archive_path" "$archive_url"
  if [[ -n "$checksum_url" ]]; then
    curl -fsSL "${auth_header[@]}" -o "$checksum_path" "$checksum_url"
    expected_checksum="$(awk '{print tolower($1)}' "$checksum_path" | head -n1)"
    if command -v sha256sum >/dev/null 2>&1; then
      computed_checksum="$(sha256sum "$archive_path" | awk '{print tolower($1)}')"
    else
      computed_checksum="$(shasum -a 256 "$archive_path" | awk '{print tolower($1)}')"
    fi
    if [[ "$expected_checksum" != "$computed_checksum" ]]; then
      echo "❌ Checksum verification failed for downloaded archive." >&2
      exit 1
    fi
  fi

  tar -xzf "$archive_path" -C "$tmp_dir"
  bundle_dir="$(find "$tmp_dir" -maxdepth 1 -type d -name 'sort-it-now-*' | head -n1)"
  if [[ -z "$bundle_dir" ]]; then
    echo "❌ Could not locate extracted release bundle." >&2
    exit 1
  fi

  INSTALL_DIR="$INSTALL_DIR" bash "$bundle_dir/install.sh"
}

BINARY_PATH="$SCRIPT_DIR/$APP_NAME"
if [[ -f "$BINARY_PATH" ]]; then
  install_local_binary "$BINARY_PATH"
else
  download_and_install_latest_release
fi
