#!/usr/bin/env bash
set -euo pipefail

APP_NAME="sort_it_now"
DEFAULT_INSTALL_DIR="/usr/local/bin"
INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY_PATH="$SCRIPT_DIR/$APP_NAME"

if [[ ! -f "$BINARY_PATH" ]]; then
  echo "❌ Konnte Binärdatei '$APP_NAME' nicht finden. Stelle sicher, dass dieses Skript im entpackten Release-Ordner ausgeführt wird." >&2
  exit 1
fi

if [[ ! -x "$BINARY_PATH" ]]; then
  echo "ℹ️ Setze Ausführungsrechte für $BINARY_PATH"
  chmod +x "$BINARY_PATH"
fi

if [[ ! -d "$INSTALL_DIR" ]]; then
  echo "ℹ️ Erstelle Installationsverzeichnis $INSTALL_DIR"
  mkdir -p "$INSTALL_DIR"
fi

if [[ ! -w "$INSTALL_DIR" ]]; then
  echo "⚠️ Schreibrechte in $INSTALL_DIR fehlen. Versuche es mit 'sudo'." >&2
  exit 1
fi

install -m 755 "$BINARY_PATH" "$INSTALL_DIR/$APP_NAME"

echo "✅ $APP_NAME wurde erfolgreich nach $INSTALL_DIR installiert."
echo "ℹ️ Starte den Dienst mit: $APP_NAME"
