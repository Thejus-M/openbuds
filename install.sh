#!/usr/bin/env bash
# Build the release binary and install it to ~/.local/bin.
set -euo pipefail

cd "$(dirname "$0")"
cargo build --release -p openbuds
install -Dm755 target/release/openbuds "$HOME/.local/bin/openbuds"
echo "Installed $HOME/.local/bin/openbuds"
