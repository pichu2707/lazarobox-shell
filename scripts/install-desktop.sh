#!/usr/bin/env bash
# Install (or remove) the Lazarobox desktop launcher for the current user.
#
# The launcher opens lazarobox-shell inside Kitty, so app launchers such as
# rofi can start it without an open terminal. Kitty and the binary are
# resolved to absolute paths at install time, because a desktop session's
# PATH often lacks ~/.cargo/bin.
#
# Usage: scripts/install-desktop.sh [--uninstall]
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
template="$repo_dir/packaging/lazarobox.desktop"

# The XDG spec says a relative XDG_DATA_HOME must be ignored.
if [[ "${XDG_DATA_HOME:-}" == /* ]]; then
  data_home="$XDG_DATA_HOME"
else
  data_home="$HOME/.local/share"
fi
apps_dir="$data_home/applications"
target="$apps_dir/lazarobox.desktop"

refresh_db() {
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$apps_dir" || true
  fi
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$target"
  refresh_db
  echo "Removed $target"
  exit 0
elif [[ $# -gt 0 ]]; then
  echo "usage: $0 [--uninstall]" >&2
  exit 2
fi

kitty="$(command -v kitty || true)"
if [[ -z "$kitty" ]]; then
  echo "error: kitty not found in PATH" >&2
  exit 1
fi

lazarobox="$(command -v lazarobox-shell || true)"
if [[ -z "$lazarobox" && -x "${CARGO_HOME:-$HOME/.cargo}/bin/lazarobox-shell" ]]; then
  lazarobox="${CARGO_HOME:-$HOME/.cargo}/bin/lazarobox-shell"
fi
if [[ -z "$lazarobox" ]]; then
  echo "error: lazarobox-shell not found; run 'cargo install --path .' first" >&2
  exit 1
fi

# Exec= needs absolute paths, and the Desktop Entry spec requires quoting for
# a set of reserved characters; refuse those paths instead of escaping them.
# Rejecting '|', '&' and '\' also keeps the sed substitution below safe.
unsafe='[][:space:]"'\''\\`$%|&;<>~*?#()]'
for path in "$kitty" "$lazarobox"; do
  if [[ "$path" != /* || "$path" =~ $unsafe ]]; then
    echo "error: unsupported path (must be absolute, no special characters): $path" >&2
    exit 1
  fi
done

# Write a temp file and rename it over the target: rofi never sees a partial
# file, and a symlinked target is replaced instead of written through.
mkdir -p "$apps_dir"
tmp="$(mktemp "$apps_dir/.lazarobox.desktop.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
sed -e "s|@KITTY@|$kitty|" -e "s|@LAZAROBOX@|$lazarobox|" "$template" >"$tmp"
chmod 644 "$tmp"
mv -f "$tmp" "$target"
refresh_db
echo "Installed $target"
echo "  Exec: $kitty --class lazarobox --title Lazarobox $lazarobox"
