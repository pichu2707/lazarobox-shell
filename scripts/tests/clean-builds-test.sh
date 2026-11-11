#!/usr/bin/env bash
# Tests for clean-builds.sh. Needs no network: every case builds a small
# repository with worktrees in a scratch directory and runs the script there.
# Usage: scripts/tests/clean-builds-test.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="$here/../clean-builds.sh"

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

pass=0
fail=0
check() { # check <name> <condition exit status>
  if [ "$2" -eq 0 ]; then
    pass=$((pass + 1))
    echo "ok   - $1"
  else
    fail=$((fail + 1))
    echo "FAIL - $1"
  fi
}

export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

# commit <dir> <file> <unix time>
commit() {
  echo "$2" > "$1/$2"
  git -C "$1" add "$2"
  GIT_AUTHOR_DATE="@$3 +0000" GIT_COMMITTER_DATE="@$3 +0000" \
    git -C "$1" commit -q -m "add $2"
}

# worktree <name> <unix time> <merged: 1/0>: a worktree on branch <name> with
# one commit at that time and a 256 KiB build folder.
worktree() {
  local dir="$scratch/wt/$1"
  git -C "$repo" worktree add -q -b "$1" "$dir" main
  commit "$dir" "$1.txt" "$2"
  mkdir -p "$dir/target/debug"
  head -c 262144 /dev/zero > "$dir/target/debug/blob"
  if [ "$3" = 1 ]; then
    git -C "$repo" merge -q --no-ff -m "merge $1" "$1"
  fi
}

# fixture: the main checkout on main and five worktrees.
fixture() {
  rm -rf "$scratch/repo" "$scratch/wt"
  repo="$scratch/repo"
  mkdir -p "$repo" "$scratch/wt"
  git -C "$repo" init -q -b main
  printf 'target/\ntarget-*/\n' > "$repo/.gitignore"
  git -C "$repo" add .gitignore
  commit "$repo" README 1000000000
  worktree old 1000000100 1
  worktree new 1000000200 1
  worktree dirty 1000000300 1
  echo wip > "$scratch/wt/dirty/wip.txt"
  worktree unmerged 1000000400 0
  worktree locked 1000000050 1
  git -C "$repo" worktree lock "$scratch/wt/locked"
}

# run <args...>: runs the script from the main checkout; sets out and code.
run() {
  code=0
  out="$(cd "$repo" && "$script" --base main "$@" 2>&1)" || code=$?
}

kib() { du -sk "$1" | awk '{ print $1 }'; }

# total_kib: what the script adds up, the main checkout and every worktree.
total_kib() {
  local sum=0 d
  for d in "$repo" "$scratch"/wt/*; do sum=$((sum + $(kib "$d"))); done
  echo "$sum"
}

has_branch() { git -C "$repo" rev-parse --verify --quiet "refs/heads/$1" > /dev/null; }

# Listing only: every state named, nothing removed.
fixture
run --cap 0K
check "a dry run exits 0" "$([ "$code" = 0 ]; echo $?)"
for state in main merged dirty unmerged locked; do
  check "the listing names the $state state" "$(grep -q " $state " <<< "$out"; echo $?)"
done
check "a build folder says it costs only a rebuild" \
  "$(grep -q 'compiler output only: removing it costs a rebuild' <<< "$out"; echo $?)"
check "a dirty worktree says why it is kept" "$(grep -q 'uncommitted changes: not removed' <<< "$out"; echo $?)"
check "an unmerged worktree says why it is kept" "$(grep -q 'commits not in main: not removed' <<< "$out"; echo $?)"
check "a dry run names what it would remove, oldest first" \
  "$(grep 'would remove' <<< "$out" | head -n 1 | grep -q '/wt/old '; echo $?)"
check "a dry run removes nothing" "$([ -d "$scratch/wt/old" ] && [ -d "$scratch/wt/new" ]; echo $?)"

# Under the cap: nothing to do, even with --apply.
fixture
run --apply --cap 100G
check "under the cap removes nothing" \
  "$(grep -q 'under the cap: nothing to remove' <<< "$out" && [ -d "$scratch/wt/old" ]; echo $?)"

# Just over the cap: only the oldest merged worktree goes, with its branch.
fixture
cap=$(($(total_kib) - 1))
run --apply --cap "${cap}K"
check "--apply exits 0" "$([ "$code" = 0 ]; echo $?)"
check "the oldest merged worktree is removed" "$([ ! -e "$scratch/wt/old" ]; echo $?)"
check "its branch is deleted" "$(! has_branch old; echo $?)"
check "a newer merged worktree is kept once under the cap" "$([ -d "$scratch/wt/new" ] && has_branch new; echo $?)"
check "git no longer lists the removed worktree" \
  "$(! git -C "$repo" worktree list | grep -q '/wt/old '; echo $?)"

# A cap of zero: every merged clean worktree goes, nothing else.
fixture
run --apply --cap 0K
check "every merged clean worktree is removed" "$([ ! -e "$scratch/wt/old" ] && [ ! -e "$scratch/wt/new" ]; echo $?)"
check "a dirty worktree is kept with its work" "$([ -f "$scratch/wt/dirty/wip.txt" ]; echo $?)"
check "an unmerged worktree and its branch are kept" "$([ -d "$scratch/wt/unmerged" ] && has_branch unmerged; echo $?)"
check "a locked worktree is kept" "$([ -d "$scratch/wt/locked" ]; echo $?)"
check "the main checkout and its build folder are kept" "$([ -d "$repo/.git" ]; echo $?)"

# Run from a merged worktree: that one is not removed.
fixture
code=0
out="$(cd "$scratch/wt/new" && "$script" --base main --apply --cap 0K 2>&1)" || code=$?
check "the worktree the script runs from is kept" \
  "$([ "$code" = 0 ] && [ -d "$scratch/wt/new" ] && grep -q ' current ' <<< "$out"; echo $?)"

# Untracked files count as work even when the user's config hides them.
fixture
git -C "$repo" config status.showUntrackedFiles no
echo notes > "$scratch/wt/old/notes.txt"
run --apply --cap 0K
check "untracked files keep a worktree with status.showUntrackedFiles=no" \
  "$([ -f "$scratch/wt/old/notes.txt" ] && grep -q 'uncommitted changes' <<< "$out"; echo $?)"

# The base's own branch, checked out in a worktree, is never "merged" into
# itself: neither the worktree nor the branch goes.
fixture
git -C "$repo" checkout -q -b side
git -C "$repo" worktree add -q "$scratch/wt/base" main
commit "$scratch/wt/base" only-here.txt 1000000500
run --apply --cap 0K
check "the worktree of the base branch is kept" "$([ -f "$scratch/wt/base/only-here.txt" ]; echo $?)"
check "the base branch is not deleted" "$(has_branch main; echo $?)"

# A binary running from inside a worktree's build folder keeps that worktree:
# deleting it would pull the executable from under the process.
fixture
worktree busy 1000000010 1
mkdir -p "$scratch/wt/busy/target/release"
cp "$(command -v sleep)" "$scratch/wt/busy/target/release/sleep"
"$scratch/wt/busy/target/release/sleep" 300 &
busy_pid=$!
sleep 0.3
run --apply --cap 0K
kill "$busy_pid" 2> /dev/null || true
wait "$busy_pid" 2> /dev/null || true
check "a worktree with a running build binary is kept" "$([ -d "$scratch/wt/busy" ] && has_branch busy; echo $?)"
check "it says a running process is the reason" \
  "$(grep -q 'running process: not removed' <<< "$out"; echo $?)"
check "it names the running binary" "$(grep -q 'target/release/sleep' <<< "$out"; echo $?)"
check "other merged worktrees are still removed" "$([ ! -e "$scratch/wt/old" ]; echo $?)"
check "--apply says it never removes build folders on their own" \
  "$(grep -q 'never removes a build folder on its own' <<< "$out"; echo $?)"

# Once the process is gone the same worktree is removable.
run --apply --cap 0K
check "a worktree is removed once its binary stopped" "$([ ! -e "$scratch/wt/busy" ]; echo $?)"

# Bad arguments stop before anything is listed.
fixture
run --cap lots
check "a bad size exits 2" "$([ "$code" = 2 ] && grep -q 'bad size' <<< "$out"; echo $?)"
run --base no-such-ref
check "a missing base ref exits 2" "$([ "$code" = 2 ] && grep -q 'base ref not found' <<< "$out"; echo $?)"
run --nope
check "an unknown argument exits 2" "$([ "$code" = 2 ]; echo $?)"

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
