#!/usr/bin/env bash
# Build the three binaries and install them to ~/.local/bin.
# Links the user service and the tray autostart entry. Installs nothing
# system-wide and does not start anything.
set -euo pipefail

cd "$(dirname "$0")"
cargo build --release -p openbuds -p openbuds-daemon -p openbuds-tray
for bin in openbuds openbudsd openbuds-tray; do
    install -Dm755 "target/release/$bin" "$HOME/.local/bin/$bin"
    echo "Installed $HOME/.local/bin/$bin"
done

mkdir -p "$HOME/.config/systemd/user" "$HOME/.config/autostart"
ln -sfn "$PWD/contrib/openbudsd.service" "$HOME/.config/systemd/user/openbudsd.service"
ln -sfn "$PWD/contrib/openbuds-tray.desktop" "$HOME/.config/autostart/openbuds-tray.desktop"
systemctl --user daemon-reload
echo "Linked the user unit and the tray autostart entry. Start with: systemctl --user start openbudsd"
