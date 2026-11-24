#!/usr/bin/env bash
# Cut a release: bump the version, commit, tag. The tag push is what starts the release workflow.
#
#   scripts/cut-release.sh alpha                    next alpha from main
#   scripts/cut-release.sh beta   [--from vX.Y.Z-alpha.N]   promote that alpha (default: newest)
#   scripts/cut-release.sh stable [--from vX.Y.Z-beta.N]    promote that beta  (default: newest)
#   scripts/cut-release.sh stable --ref release/0.1 --base 0.1.1   hotfix: tag the tip of that branch (docs/releasing.md)
# Options: --ref BRANCH (hotfix: cut from that branch), --base X.Y.Z (override the number), --push (push branch and tag; otherwise it only
# prints the command), --dry-run (print the version and stop).
# Promotions branch from the tested tag, so the new tag is that exact build plus a version bump.
set -euo pipefail
cd "$(dirname "$0")/.."
# Checking out another branch or an older tag swaps the scripts folder, so run from copies.
T=$(mktemp -d); orig=$(git rev-parse --abbrev-ref HEAD)
trap 'rm -rf "$T"; [ "$orig" = HEAD ] || git checkout -q "$orig" 2>/dev/null || true' EXIT
cp scripts/release-version.sh scripts/bump-version.sh "$T/"
export SPRK_ROOT="$PWD"
rv="$T/release-version.sh"; bump="$T/bump-version.sh"
kind="${1:?usage: cut-release.sh <alpha|beta|stable> [--from TAG] [--base X.Y.Z] [--push] [--dry-run]}"; shift
from=""; base=""; ref=""; push=0; dry=0
while [ $# -gt 0 ]; do
  case "$1" in --from) from="$2"; shift 2 ;; --base) base="$2"; shift 2 ;; --ref) ref="$2"; shift 2 ;; --push) push=1; shift ;; --dry-run) dry=1; shift ;; *) echo "unknown option $1" >&2; exit 2 ;; esac
done
git diff --quiet && git diff --cached --quiet || { echo "working tree not clean" >&2; exit 1; }
git fetch -q --tags origin 2>/dev/null || true

args=(); [ -n "$base" ] && args+=(--base "$base"); [ -n "$from" ] && args+=(--from "$from")
[ -z "$ref" ] || [ -n "$base" ] || { echo "--ref needs --base (the new version number)" >&2; exit 2; }
ver=$("$rv" next "$kind" ${args[@]+"${args[@]}"}); tag="v$ver"
echo "Next $kind version: $ver (tag $tag)"
[ "$dry" = 1 ] && exit 0
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && { echo "$tag already exists" >&2; exit 1; }

if [ -n "$ref" ]; then
  [ "$kind" != alpha ] || { echo "--ref is for hotfixes (beta or stable)" >&2; exit 2; }
  branch="$ref"; git checkout -q "$branch"; git merge -q --ff-only "origin/$branch" 2>/dev/null || true
elif [ "$kind" = alpha ]; then
  branch=main
  git checkout -q main; git merge -q --ff-only origin/main 2>/dev/null || true
else
  [ -n "$from" ] || from=$(git tag -l 'v*-alpha.*' 'v*-beta.*' | sed 's/^v//' | sort -V | tail -1 | sed 's/^/v/')
  [ -n "$from" ] || { echo "nothing to promote: pass --from" >&2; exit 1; }
  branch="release/${ver%%-*}"
  if git rev-parse -q --verify "refs/heads/$branch" >/dev/null || git rev-parse -q --verify "refs/remotes/origin/$branch" >/dev/null; then
    git checkout -q "$branch"
    # Already holds earlier builds of this release: the new commit must sit on the tested tag.
    git merge-base --is-ancestor "$from" HEAD || { echo "$branch does not contain $from" >&2; exit 1; }
  else
    git checkout -q -b "$branch" "$from"
  fi
fi

"$bump" "$ver"
# Cargo.lock records the workspace crates' versions; keep it in step so --locked builds pass.
cargo update --workspace >/dev/null 2>&1
(cd app/src-tauri && cargo update --workspace >/dev/null 2>&1)
"$bump" --check
git -c user.email=noreply@pwrs.dev -c user.name="SprkNotes release" commit -qam "chore(release): $tag"
git -c user.email=noreply@pwrs.dev -c user.name="SprkNotes release" tag -a "$tag" -m "SprkNotes $ver"
"$rv" guard "$tag"
echo "Created $tag on $branch."
if [ "$push" = 1 ]; then git push origin "$branch" "$tag"; echo "Pushed. The release workflow now builds the draft."
else echo "To publish: git push origin $branch $tag"; fi
