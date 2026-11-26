#!/usr/bin/env bash
# Builds Ask Sprk model packs, splits them into 80 MB parts (the host rejects bigger uploads),
# merges them into the models index, and uploads everything to the "models" release on GitHub
# (the default) or on a Forgejo host (SPRK_HOST and SPRK_REPO).
#
#   scripts/publish-assistant-model.sh <work dir> <pack folder>...
#   DRY_RUN=1 scripts/publish-assistant-model.sh ...   # build and merge only, upload nothing
#
# A <pack folder> is what scripts/make-assistant-pack-folder.py writes (model.json + model.gguf).
# Needs: sprkctl on PATH (or SPRKCTL=...), python3, curl, git. On GitHub uploads use the gh CLI
# (GH_TOKEN in CI). On Forgejo they use your stored git credential for the host (or SPRK_TOKEN,
# an access token with write:repository), and every HTTP status is checked.
set -euo pipefail
work=${1:?work dir}; shift
[ $# -ge 1 ] || { echo "give at least one pack folder"; exit 2; }
SPRKCTL=${SPRKCTL:-sprkctl}
H=${SPRK_HOST:-https://github.com}; R=${SPRK_REPO:-ConniptionFit/SprkNotes}; TAG=${SPRK_MODELS_TAG:-models}
BASE="$H/$R/releases/download/$TAG"
mkdir -p "$work/packs" "$work/parts"; cd "$work"

packs=()
for dir in "$@"; do
  id=$(python3 -c 'import json,sys;m=json.load(open(sys.argv[1]+"/model.json"));print(m["id"]+"@"+m["version"])' "$dir")
  "$SPRKCTL" model pack "$dir" -o "packs/$id.sprkmodel"
  packs+=("packs/$id.sprkmodel")
done

# New index lines for the new packs, with parts. Existing lines are kept unless replaced.
"$SPRKCTL" model index "${packs[@]}" --base-url "$BASE" --url-suffix .zip --split-mb 80 --parts-dir parts -o new-index.json
if [ -n "${DRY_RUN:-}" ] && [ -f "${OLD_INDEX:-}" ]; then cp "$OLD_INDEX" old-index.json
else curl -fsSL "$BASE/models-index.json" -o old-index.json || echo '{"schema":1,"models":[]}' > old-index.json; fi
python3 - <<'PY'
import json
old, new = json.load(open("old-index.json")), json.load(open("new-index.json"))
keep = [m for m in old["models"] if (m["id"], m["version"]) not in {(n["id"], n["version"]) for n in new["models"]}]
json.dump({"schema": 1, "models": keep + new["models"]}, open("models-index.json", "w"), indent=2)
print(f"index: kept {len(keep)}, added {len(new['models'])}")
PY
ls -l parts | head -50
[ -n "${DRY_RUN:-}" ] && { echo "dry run: nothing uploaded. Files are in $work"; exit 0; }

if [ "$H" = https://github.com ]; then
  # A file of the same name is replaced. The index goes last, so the app never sees entries whose
  # parts are not there yet.
  for f in parts/*; do gh release upload "$TAG" "$f" --clobber -R "$R"; echo "ok   $(basename "$f")"; done
  gh release upload "$TAG" models-index.json --clobber -R "$R"
  echo "Published. In the app: Settings > Ask Sprk > Download."
  exit 0
fi

if [ -n "${SPRK_TOKEN:-}" ]; then AUTH=(-H "Authorization: token $SPRK_TOKEN")
else
  cred=$(printf 'protocol=https\nhost=%s\n\n' "${H#https://}" | git credential fill)
  AUTH=(-u "$(sed -n 's/^username=//p' <<<"$cred"):$(sed -n 's/^password=//p' <<<"$cred")")
fi
rel=$(curl -fsS "${AUTH[@]}" "$H/api/v1/repos/$R/releases/tags/$TAG"); ID=$(python3 -c 'import json,sys;print(json.load(sys.stdin)["id"])' <<<"$rel")
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
printf '%s' "$rel" > "$tmp/rel.json"
up() { # file, uploaded name. A file of the same name is replaced.
  old=$(python3 -c 'import json,sys;print(next((a["id"] for a in json.load(open(sys.argv[1]))["assets"] if a["name"]==sys.argv[2]), ""))' "$tmp/rel.json" "$2")
  [ -n "$old" ] && curl -fsS -X DELETE "${AUTH[@]}" "$H/api/v1/repos/$R/releases/$ID/assets/$old" >/dev/null
  code=$(curl -s -o "$tmp/out" -w '%{http_code}' "${AUTH[@]}" -F "attachment=@$1;filename=$2" "$H/api/v1/repos/$R/releases/$ID/assets?name=$2")
  [ "$code" = 201 ] && echo "ok   $2" || { echo "FAIL $2 (HTTP $code)"; cat "$tmp/out"; exit 1; }
}
for f in parts/*; do up "$f" "$(basename "$f")"; done
# The index goes last, so the app never sees entries whose parts are not there yet.
up models-index.json models-index.json
echo "Published. In the app: Settings > Ask Sprk > Download."
