#!/usr/bin/env bash
# Thin wrapper kept for repo clones: the launcher logic lives in the binary.
#
# Usage: scripts/install-desktop.sh [--terminal <cmd>] [--uninstall]
# Same as: lazarobox-shell install-desktop [--terminal <cmd>] [--uninstall]
set -euo pipefail

bin="$(command -v lazarobox-shell || true)"
if [[ -z "$bin" && -x "${CARGO_HOME:-$HOME/.cargo}/bin/lazarobox-shell" ]]; then
  bin="${CARGO_HOME:-$HOME/.cargo}/bin/lazarobox-shell"
fi
if [[ -z "$bin" ]]; then
  echo "error: lazarobox-shell not found; run 'cargo install --path .' first" >&2
  exit 1
fi

exec "$bin" install-desktop "$@"
