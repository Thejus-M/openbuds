#!/usr/bin/env bash
# Stop the old Galaxy Buds stack and start openbuds in its place.
# Saves the old bar layout first. contrib/swap-back.sh restores it.
set -euo pipefail

BACKUP_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/openbuds/swap"
SHELL_JSON="$HOME/.config/omarchy/shell.json"
mkdir -p "$BACKUP_DIR"
cp "$SHELL_JSON" "$BACKUP_DIR/shell.json"

systemctl --user stop galaxybuds.service || true

# Remove the home.buds widget from the bar layout (exact JSON edit).
python3 - "$SHELL_JSON" <<'PY'
import json, sys
path = sys.argv[1]
with open(path) as f:
    data = json.load(f)
def strip(node):
    if isinstance(node, dict):
        for key, value in list(node.items()):
            if isinstance(value, list):
                node[key] = [v for v in value if not (isinstance(v, dict) and v.get("id") == "home.buds")]
                for v in node[key]:
                    strip(v)
            elif isinstance(value, dict):
                strip(value)
strip(data)
with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
PY

omarchy-restart-shell
systemctl --user start openbudsd.service
nohup "$HOME/.local/bin/openbuds-tray" >/dev/null 2>&1 &
echo "Swapped. Backup: $BACKUP_DIR/shell.json"
