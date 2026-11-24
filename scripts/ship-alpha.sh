#!/usr/bin/env bash
# Ship an alpha from the RX9070: update main, cut the tag, build Windows on sprk-winvm from that
# exact tag, upload and publish. Run it with bash, so it works from fish too:
#
#   bash scripts/ship-alpha.sh                       cut the next alpha from main and ship it
#   bash scripts/ship-alpha.sh --base 0.2.0          choose the version number (first alpha of 0.2.0)
#   bash scripts/ship-alpha.sh --wait-pr 80          wait (up to 60 min) for that PR to merge first
#   bash scripts/ship-alpha.sh --tag v0.2.0-alpha.1  resume: build and upload an existing tag
#   LIGHT_PACK_DIR=/path/to/pack-folder bash scripts/ship-alpha.sh ...   also publish the Ask Sprk model pack
#
# Needs: the release token in ~/.config/sprknotes/forgejo-token (see docs/releasing.md), ssh access
# to sprk-winvm (SPRK_WINVM), and your git credentials for pushing the tag. Stops on the first error.
set -euo pipefail
cd "$(dirname "$0")/.."
REPO=$PWD
API=https://git.pwrs.dev/api/v1/repos/powers/sprknotes
VM=${SPRK_WINVM:-sprk-winvm}
base=""; tag=""; waitpr=""
while [ $# -gt 0 ]; do
  case "$1" in --base) base="$2"; shift 2 ;; --tag) tag="$2"; shift 2 ;; --wait-pr) waitpr="$2"; shift 2 ;; *) echo "unknown option $1" >&2; exit 2 ;; esac
done

echo "== 1. Token"
[ -r "$HOME/.config/sprknotes/forgejo-token" ] || { cat >&2 <<'MSG'
Missing ~/.config/sprknotes/forgejo-token. One time:
  Forgejo > avatar > Settings > Applications > new token, name sprknotes-release, scope write:repository only.
  mkdir -p ~/.config/sprknotes && printf '%s\n' 'PASTE_TOKEN_HERE' > ~/.config/sprknotes/forgejo-token && chmod 600 ~/.config/sprknotes/forgejo-token
Then run this again.
MSG
exit 1; }

echo "== 2. Update the checkout"
git fetch -q --tags origin
# Only the Cargo.lock files may be dirty; anything else stops here so no work is lost.
other=$(git status --porcelain | grep -vE '^( M|M | MM|MM) (app/src-tauri/)?Cargo\.lock$' || true)
[ -z "$other" ] || { echo "Uncommitted changes besides Cargo.lock, not touching them:" >&2; echo "$other" >&2; exit 1; }
if ! git diff --quiet -- Cargo.lock app/src-tauri/Cargo.lock; then
  git stash push -m "cargo.lock before alpha $(date +%F)" -- Cargo.lock app/src-tauri/Cargo.lock
  echo "(Cargo.lock changes saved: see git stash list)"
fi
git switch -q main
git merge -q --ff-only origin/main

if [ -n "$waitpr" ]; then
  echo "== 3. Wait for PR $waitpr to merge"
  for i in $(seq 1 120); do
    merged=$(curl -fsS "$API/pulls/$waitpr" | python3 -c 'import json,sys;print(json.load(sys.stdin)["merged"])')
    [ "$merged" = True ] && break
    [ "$i" = 1 ] && echo "PR $waitpr is not merged yet. Checking every 30 s, up to 60 min..."
    sleep 30
  done
  [ "$merged" = True ] || { echo "PR $waitpr is still not merged. Run this again after it lands." >&2; exit 1; }
  git fetch -q --tags origin; git merge -q --ff-only origin/main
fi

echo "== 4. Cut the alpha"
if [ -z "$tag" ]; then
  args=(); [ -n "$base" ] && args+=(--base "$base")
  scripts/cut-release.sh alpha ${args[@]+"${args[@]}"} --push
  git fetch -q --tags origin
  tag="v$(git tag -l 'v*-alpha.*' --sort=-creatordate | head -1 | sed 's/^v//')"
fi
git rev-parse -q --verify "refs/tags/$tag" >/dev/null || { echo "no tag $tag" >&2; exit 1; }
echo "Shipping $tag"

echo "== 5. Windows libraries to the VM"
scripts/fetch-sherpa.sh win-x64-shared win-arm64-shared
X64=$(scripts/fetch-sherpa.sh --print win-x64-shared); ARM=$(scripts/fetch-sherpa.sh --print win-arm64-shared)
WINHOME=$(ssh -o BatchMode=yes "$VM" 'powershell -NoProfile -Command "(Resolve-Path ~).Path"' | tr -d '\r'); WINHOME=${WINHOME//\\//}
ssh -o BatchMode=yes "$VM" 'powershell -NoProfile -Command "New-Item -Force -ItemType Directory ~/sprk-sherpa/x64, ~/sprk-sherpa/arm64 | Out-Null"'
scp -q -r -o BatchMode=yes "$X64/." "$VM:sprk-sherpa/x64/"
scp -q -r -o BatchMode=yes "$ARM/." "$VM:sprk-sherpa/arm64/"

echo "== 6. Build on the VM from the tag (the long step)"
scripts/winvm.sh release-tag "$tag" -SherpaX64 "$WINHOME/sprk-sherpa/x64" -SherpaArm64 "$WINHOME/sprk-sherpa/arm64"
git switch -q main
scripts/winvm.sh dist

echo "== 7. Draft, upload, publish"
scripts/release-notes.sh "$tag" > "/tmp/notes-$tag.md"
scripts/publish-release.sh draft "$tag" --notes "/tmp/notes-$tag.md"
scripts/publish-release.sh all "$tag" ./dist-win --publish

echo "== 8. Ask Sprk model pack (optional)"
if [ -n "${LIGHT_PACK_DIR:-}" ] && [ -d "$LIGHT_PACK_DIR" ]; then
  cargo build --release -p sprkctl
  SPRKCTL="$REPO/target/release/sprkctl" scripts/publish-assistant-model.sh "$HOME/sprknotes-dist/assistant-model" "$LIGHT_PACK_DIR"
else
  echo "skipped (set LIGHT_PACK_DIR=/path/to/pack-folder to publish the model pack too)"
fi
echo "Done: $tag is published. On the laptop: Settings > Updates > Check for updates."
