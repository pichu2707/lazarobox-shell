#!/usr/bin/env bash
# Lists the git worktrees and build folders of this repository with their size,
# branch and merge state, and says what removing each one affects. With
# --apply it removes the oldest merged worktrees, with their build folders and
# local branches, until the total is under the size cap.
#
# Never touched: the main checkout, the worktree this runs from, a locked
# worktree, the worktree of the base branch, a worktree with uncommitted
# changes (untracked files included, whatever the git config), a worktree
# whose commits are not all in the base ref, and a worktree with a running
# process whose executable lives inside it (a release binary in target/). That
# check reads /proc on Linux and `ps` elsewhere; when neither works it says so.
# Build folders are only listed, never removed on their own: removing one
# costs a rebuild and nothing else (`cargo clean`).
#
# Usage: scripts/clean-builds.sh [--apply] [--cap SIZE] [--base REF]
#   --apply     remove instead of only listing
#   --cap SIZE  total size to stay under, as a number with K, M or G
#               (default 40G); merged worktrees are removed oldest first
#   --base REF  where merged commits are (default origin/main); run
#               `git fetch` first so it is current
set -euo pipefail

usage() {
  sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'
}

apply=0
cap="40G"
base="origin/main"
while [ $# -gt 0 ]; do
  case "$1" in
    --apply) apply=1 ;;
    --cap) cap="${2:?--cap needs a size}"; shift ;;
    --base) base="${2:?--base needs a ref}"; shift ;;
    -h | --help) usage; exit 0 ;;
    *) echo "clean-builds: unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

# to_kib <size>: 40G, 512M, 100K or a plain number of KiB.
to_kib() {
  local n="${1%[KkMmGg]}" unit="${1#"${1%[KkMmGg]}"}"
  case "$n" in '' | *[!0-9]*) echo "clean-builds: bad size: $1" >&2; exit 2 ;; esac
  case "$unit" in
    G | g) echo $((n * 1024 * 1024)) ;;
    M | m) echo $((n * 1024)) ;;
    *) echo "$n" ;;
  esac
}

# human <kib>
human() {
  local k="$1"
  if [ "$k" -ge 1048576 ]; then
    awk -v k="$k" 'BEGIN { printf "%.1fG", k / 1048576 }'
  elif [ "$k" -ge 1024 ]; then
    echo "$((k / 1024))M"
  else
    echo "${k}K"
  fi
}

# size_kib <path>: 0 when it is gone.
size_kib() {
  if [ -e "$1" ]; then du -sk "$1" 2>/dev/null | awk '{ print $1 }'; else echo 0; fi
}

# running_in <dir>: prints one running executable under <dir> and succeeds, or
# fails when there is none. Sets proc_check=0 when no way to list running
# executables works here, so the listing can say the check was skipped.
proc_check=1
running_in() {
  local dir="${1%/}" link exe
  if [ -r /proc/self/exe ]; then
    for link in /proc/[0-9]*/exe; do
      exe="$(readlink "$link" 2> /dev/null)" || continue
      exe="${exe% (deleted)}"
      case "$exe" in "$dir"/*) echo "$exe"; return 0 ;; esac
    done
    return 1
  fi
  if exe="$(ps -axo comm= 2> /dev/null)"; then
    exe="$(printf '%s\n' "$exe" | awk -v d="$dir/" 'index($0, d) == 1 { print; exit }')"
    [ -n "$exe" ] && { echo "$exe"; return 0; }
    return 1
  fi
  proc_check=0
  return 1
}

cap_kib="$(to_kib "$cap")"
git rev-parse --git-dir > /dev/null 2>&1 || { echo "clean-builds: not in a git repository" >&2; exit 2; }
git rev-parse --verify --quiet "$base^{commit}" > /dev/null \
  || { echo "clean-builds: base ref not found: $base" >&2; exit 2; }
here="$(git rev-parse --show-toplevel)"
# The base's own branch is in the base by definition: never "merged".
base_branch="$(git rev-parse --symbolic-full-name "$base")"

# One line per worktree: path, branch (or "-"), locked (0/1). The first one
# git lists is the main checkout.
worktrees="$(git worktree list --porcelain | awk '
  /^worktree / { if (path != "") print path "\t" branch "\t" locked; path = substr($0, 10); branch = "-"; locked = 0 }
  /^branch /   { branch = substr($0, 8); sub("^refs/heads/", "", branch) }
  /^locked/    { locked = 1 }
  END          { if (path != "") print path "\t" branch "\t" locked }')"
main_path="$(printf '%s\n' "$worktrees" | head -n 1 | cut -f 1)"

total=0
candidates="" # "<commit time>\t<kib>\t<path>\t<branch>" of removable worktrees
printf '%-8s %8s  %-10s %-34s %s\n' KIND SIZE STATE BRANCH PATH

while IFS="$(printf '\t')" read -r path branch locked; do
  [ -n "$path" ] || continue
  kib="$(size_kib "$path")"
  total=$((total + kib))
  if [ "$path" = "$main_path" ]; then
    state="main"
    effect="the main checkout: never removed"
  elif [ "$path" = "$here" ]; then
    state="current"
    effect="this script runs here: not removed"
  elif [ "$locked" = 1 ]; then
    state="locked"
    effect="locked worktree: not removed"
  elif [ "refs/heads/$branch" = "$base_branch" ]; then
    state="base"
    effect="holds the base branch: not removed"
  elif [ ! -d "$path" ]; then
    state="missing"
    effect="folder is gone: run git worktree prune"
  elif [ -n "$(git -C "$path" status --porcelain --untracked-files=normal 2> /dev/null)" ]; then
    state="dirty"
    effect="uncommitted changes: not removed"
  elif ! git merge-base --is-ancestor "$(git -C "$path" rev-parse HEAD)" "$base"; then
    state="unmerged"
    effect="commits not in $base: not removed"
  elif running="$(running_in "$path")"; then
    state="running"
    effect="running process: not removed ($running)"
  else
    state="merged"
    effect="its commits are in $base; ignored files (build output) go with it"
    when="$(git -C "$path" log -1 --format=%ct HEAD)"
    candidates="$candidates$when	$kib	$path	$branch
"
  fi
  printf '%-8s %8s  %-10s %-34s %s\n' worktree "$(human "$kib")" "$state" "$branch" "$path"
  printf '%-8s %8s  %-10s %-34s   %s\n' "" "" "" "" "$effect"
  for build in "$path"/target "$path"/target-*; do
    [ -d "$build" ] || continue
    printf '%-8s %8s  %-10s %-34s %s\n' build "$(human "$(size_kib "$build")")" "" "" "$build"
    printf '%-8s %8s  %-10s %-34s   %s\n' "" "" "" "" "compiler output only: removing it costs a rebuild"
  done
done <<EOF
$worktrees
EOF

echo
if [ "$proc_check" = 0 ]; then
  echo "note: cannot list running processes here: a running binary is not detected"
fi
echo "total $(human "$total"), cap $(human "$cap_kib")"
if [ "$total" -le "$cap_kib" ]; then
  echo "under the cap: nothing to remove"
  exit 0
fi
if [ -z "$candidates" ]; then
  echo "over the cap, but no merged clean worktree to remove"
  echo "the build folders listed above are compiler output: \`cargo clean\` in a checkout frees its own"
  exit 0
fi

removed=0
while IFS="$(printf '\t')" read -r _ kib path branch; do
  [ -n "$path" ] || continue
  [ "$total" -gt "$cap_kib" ] || break
  if [ "$apply" = 0 ]; then
    echo "would remove $path ($(human "$kib"))"
  else
    git worktree remove "$path"
    # Its commits are in the base: -D is safe, and -d would compare with
    # whatever the main checkout has checked out instead of the base.
    if [ "$branch" != "-" ] && git merge-base --is-ancestor "$branch" "$base"; then
      git branch -D "$branch" > /dev/null
    fi
    echo "removed $path ($(human "$kib"))"
  fi
  removed=$((removed + 1))
  total=$((total - kib))
done <<EOF
$(printf '%s' "$candidates" | sort -n)
EOF

echo "never removes a build folder on its own: it goes only with its merged worktree"
if [ "$apply" = 0 ]; then
  echo "dry run: pass --apply to remove $removed worktree(s)"
else
  git worktree prune
  echo "total now $(human "$total")"
fi
