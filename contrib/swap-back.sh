#!/usr/bin/env bash
# Undo contrib/swap-to-rust.sh: stop openbuds, restore the old stack.
set -euo pipefail

BACKUP_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/openbuds/swap"
SHELL_JSON="$HOME/.config/omarchy/shell.json"

systemctl --user stop openbudsd.service || true
pkill -x openbuds-tray || true

cp "$BACKUP_DIR/shell.json" "$SHELL_JSON"
systemctl --user start galaxybuds.service
omarchy-restart-shell
echo "Restored the old Galaxy Buds stack."
