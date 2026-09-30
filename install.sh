#!/bin/bash
# Builds agentboard and installs it to ~/.local/bin. Pass --watch to also enable the notification service.
set -euo pipefail
cd "$(dirname "$0")"

command -v cargo >/dev/null || { echo "cargo not found: install Rust first (https://rustup.rs)" >&2; exit 1; }
cargo build --release --quiet
install -Dm755 target/release/agentboard "$HOME/.local/bin/agentboard"
echo "installed ~/.local/bin/agentboard"

if [[ "${1:-}" == "--watch" ]]; then
  install -Dm644 contrib/agentboard-watch.service "$HOME/.config/systemd/user/agentboard-watch.service"
  systemctl --user daemon-reload
  systemctl --user enable --now agentboard-watch.service
  echo "notifications enabled (systemctl --user status agentboard-watch)"
fi

case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) echo "note: ~/.local/bin is not on your PATH" >&2 ;; esac
