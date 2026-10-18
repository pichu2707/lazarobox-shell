#!/usr/bin/env bash
# Print the body of one version's section from CHANGELOG.md: the lines after
# `## [X.Y.Z] - date` up to (not including) the next `## [` heading or the
# link-reference footer (`[X.Y.Z]: url`).
# Usage: changelog-section.sh <version> [changelog]
# Exits 1 when the version has no section or its body is empty.
set -euo pipefail

version="${1:?usage: changelog-section.sh <version> [changelog]}"
changelog="${2:-CHANGELOG.md}"

notes="$(awk -v ver="$version" '
  index($0, "## [" ver "]") == 1 { found = 1; next }
  found && (index($0, "## [") == 1 || $0 ~ /^\[[^]]+\]: /) { exit }
  found { print }
' "$changelog")"

# Trim leading and trailing blank lines.
notes="$(printf '%s\n' "$notes" | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}')"

if [ -z "$notes" ]; then
  echo "No CHANGELOG section for version $version in $changelog" >&2
  exit 1
fi
printf '%s\n' "$notes"
