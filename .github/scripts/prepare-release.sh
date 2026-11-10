#!/usr/bin/env bash
# Prepare a release in the working tree: pick the next version, move the
# `[Unreleased]` entries of CHANGELOG.md under it, and bump Cargo.toml and
# Cargo.lock. It touches no git state and needs no network; the release
# workflow commits and tags the result.
# Usage: prepare-release.sh <auto|patch|minor|major> [repo_root] [date]
#   repo_root defaults to `.`; date defaults to today (UTC), as YYYY-MM-DD.
# Bump rules:
#   auto   `minor` when [Unreleased] has a `### Added` heading, else `patch`.
#          It never picks `major`: a breaking change is a decision to make.
#   A pre-release version (1.1.0-rc.1) is treated through its base (1.1.0):
#   `auto` and `patch` finalize it to 1.1.0; `minor` and `major` bump from the
#   base, as if it had been released (1.2.0 and 2.0.0).
# The new version is the last line printed on stdout.
set -euo pipefail

bump="${1:?usage: prepare-release.sh <auto|patch|minor|major> [repo_root] [date]}"
root="${2:-.}"
date="${3:-$(date -u +%F)}"

case "$bump" in
  auto | patch | minor | major) ;;
  *) echo "Unknown bump '$bump': use auto, patch, minor or major" >&2; exit 1 ;;
esac

cargo_toml="$root/Cargo.toml"
cargo_lock="$root/Cargo.lock"
changelog="$root/CHANGELOG.md"
for f in "$cargo_toml" "$cargo_lock" "$changelog"; do
  [ -f "$f" ] || { echo "Missing $f" >&2; exit 1; }
done

# `version` and `name` of the [package] table, the first one in the file (same
# approach as release.yml).
package_field() {
  awk -F'"' -v key="$1" '/^\[package\]/ {p=1; next} /^\[/ {p=0} p && $0 ~ "^" key "[[:space:]]*=" {print $2; exit}' "$cargo_toml"
}
current="$(package_field version)"
name="$(package_field name)"
if [ -z "$current" ] || [ -z "$name" ]; then
  echo "Could not read the package name and version from $cargo_toml" >&2
  exit 1
fi
if ! [[ "$current" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)(-.+)?$ ]]; then
  echo "Current version '$current' is not SemVer" >&2
  exit 1
fi
major="${BASH_REMATCH[1]}" minor="${BASH_REMATCH[2]}" patch="${BASH_REMATCH[3]}"
prerelease="${BASH_REMATCH[4]}"

# The body of [Unreleased]: up to the next `## [` heading or the link footer.
unreleased="$(awk '
  /^## \[Unreleased\]/ { found = 1; next }
  found && (/^## \[/ || /^\[[^]]+\]: /) { exit }
  found { print }
' "$changelog")"

# Headings and blank lines are not entries.
if ! grep -qvE '^([[:space:]]*|### .*)$' <<<"$unreleased"; then
  echo "[Unreleased] in $changelog has no entries; nothing to release" >&2
  exit 1
fi

if [ "$bump" = auto ]; then
  if grep -qE '^### Added[[:space:]]*$' <<<"$unreleased"; then bump="minor"; else bump="patch"; fi
fi

case "$bump" in
  major) new="$((major + 1)).0.0" ;;
  minor) new="${major}.$((minor + 1)).0" ;;
  patch)
    if [ -n "$prerelease" ]; then new="${major}.${minor}.${patch}"; else new="${major}.${minor}.$((patch + 1))"; fi
    ;;
esac
# `auto` on a pre-release finalizes it, whatever the entries say.
if [ -n "$prerelease" ] && [ "$1" = auto ]; then new="${major}.${minor}.${patch}"; fi

# The previous tag is the base of the existing [Unreleased] compare link.
unreleased_link="$(grep -m1 '^\[Unreleased\]: ' "$changelog" || true)"
if ! [[ "$unreleased_link" =~ ^\[Unreleased\]:\ (.*)/compare/(v.+)\.\.\.HEAD$ ]]; then
  echo "Could not find the '[Unreleased]: .../compare/vX.Y.Z...HEAD' footer link in $changelog" >&2
  exit 1
fi
repo_url="${BASH_REMATCH[1]}"
previous_tag="${BASH_REMATCH[2]}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

awk -v new="$new" -v date="$date" -v repo="$repo_url" -v prev="$previous_tag" '
  function flush() {
    print "## [Unreleased]"
    print ""
    print "## [" new "] - " date
    for (i = 1; i <= n; i++) print buf[i]
    if (n > 0 && buf[n] != "") print ""
  }
  state == 0 && /^## \[Unreleased\]/ { state = 1; next }
  state == 1 && (/^## \[/ || /^\[[^]]+\]: /) { flush(); state = 2 }
  state == 1 { buf[++n] = $0; next }
  /^\[Unreleased\]: / {
    print "[Unreleased]: " repo "/compare/v" new "...HEAD"
    print "[" new "]: " repo "/compare/" prev "...v" new
    next
  }
  { print }
  END { if (state == 1) flush() }
' "$changelog" > "$tmp/CHANGELOG.md"

# Only the [package] version line of Cargo.toml.
awk -v new="$new" '
  /^\[package\]/ { p = 1; print; next }
  /^\[/ { p = 0 }
  p && !done && /^version[[:space:]]*=/ { print "version = \"" new "\""; done = 1; next }
  { print }
' "$cargo_toml" > "$tmp/Cargo.toml"

# Only the version line that follows `name = "<package>"` in Cargo.lock.
awk -v new="$new" -v name="$name" '
  $0 == "name = \"" name "\"" { hit = 1; print; next }
  hit && /^version = / { print "version = \"" new "\""; hit = 0; next }
  /^\[\[package\]\]/ { hit = 0 }
  { print }
' "$cargo_lock" > "$tmp/Cargo.lock"

if ! grep -qxF "version = \"$new\"" "$tmp/Cargo.lock" || cmp -s "$cargo_lock" "$tmp/Cargo.lock"; then
  echo "Cargo.lock has no '$name' entry to update" >&2
  exit 1
fi

# cat keeps the original files' modes.
cat "$tmp/CHANGELOG.md" > "$changelog"
cat "$tmp/Cargo.toml" > "$cargo_toml"
cat "$tmp/Cargo.lock" > "$cargo_lock"

echo "Prepared $name $current -> $new ($date)" >&2
echo "$new"
