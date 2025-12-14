#!/usr/bin/env bash
set -euo pipefail

APP_NAME="sort_it_now"
DEFAULT_INSTALL_DIR="/usr/local/bin"
INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY_PATH="$SCRIPT_DIR/$APP_NAME"

if [[ ! -f "$BINARY_PATH" ]]; then
  echo "❌ Could not find binary '$APP_NAME'. Make sure this script is run in the extracted release folder." >&2
  exit 1
fi

if [[ ! -x "$BINARY_PATH" ]]; then
  echo "ℹ️ Setting execute permissions for $BINARY_PATH"
  chmod +x "$BINARY_PATH"
fi

if [[ ! -d "$INSTALL_DIR" ]]; then
  echo "ℹ️ Creating installation directory $INSTALL_DIR"
  mkdir -p "$INSTALL_DIR"
fi

if [[ ! -w "$INSTALL_DIR" ]]; then
  echo "⚠️ Write permissions missing in $INSTALL_DIR. Try using 'sudo'." >&2
  exit 1
fi

install -m 755 "$BINARY_PATH" "$INSTALL_DIR/$APP_NAME"

echo "✅ $APP_NAME was successfully installed to $INSTALL_DIR."
echo "ℹ️ Start the service with: $APP_NAME"
