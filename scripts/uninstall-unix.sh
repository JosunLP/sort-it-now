#!/usr/bin/env bash
set -euo pipefail

APP_NAME="sort_it_now"
DEFAULT_INSTALL_DIR="/usr/local/bin"
INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
BINARY_PATH="$INSTALL_DIR/$APP_NAME"

if [[ ! -e "$BINARY_PATH" ]]; then
  echo "ℹ️ $APP_NAME is not installed in $INSTALL_DIR."
  exit 0
fi

if [[ ! -w "$INSTALL_DIR" ]]; then
  echo "⚠️ Write permissions missing in $INSTALL_DIR. Try using 'sudo'." >&2
  exit 1
fi

rm -f "$BINARY_PATH"
echo "✅ $APP_NAME was successfully removed from $INSTALL_DIR."
