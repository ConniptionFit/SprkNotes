#!/usr/bin/env bash
# Create, fill and publish a Forgejo release with a scoped token. Never prints the token.
#
#   publish-release.sh draft   <tag> [--notes FILE]     create (or update) the draft; pre-release flag from the tag
#   publish-release.sh upload  <tag> <file>...          one request per file; replaces a same-named asset; checks size
#   publish-release.sh all     <tag> <dir> [--publish]  every top-level file in <dir> (e.g. the Windows dist folder)
#   publish-release.sh publish <tag>                    draft -> published (checks the update manifest is attached)
#   publish-release.sh yank    <tag>                    published -> draft: new installs and update checks stop seeing it
#   publish-release.sh status  <tag>                    state and asset list
#
# Token: FORGEJO_TOKEN, or the first line of ~/.config/sprknotes/forgejo-token. Create it in Forgejo
# (Settings, Applications) with only the "write:repository" scope. Repository: distribution.toml,
# or SPRK_REPO_URL. Forgejo takes only some file types as attachments (.zip, .json, .txt, .gz, .md and a few
# more), at most 5 per request and 2 GB each, and Cloudflare in front rejects bodies over about
# 100 MB, so this script sends one file per request and gives other types a .zip suffix (the
# update manifest records those names). The caller must keep files under the size limit.
set -euo pipefail
cd "$(dirname "$0")/.."
cmd="${1:-}"; shift || true
repo_url="${SPRK_REPO_URL:-$(sed -n 's/^repo_url *= *"\(.*\)"/\1/p' distribution.toml)}"
host="${repo_url%/*/*}"; slug="${repo_url#"$host"/}"
api="$host/api/v1/repos/$slug"
token="${FORGEJO_TOKEN:-}"
[ -n "$token" ] || { f="$HOME/.config/sprknotes/forgejo-token"; [ -r "$f" ] && token=$(head -1 "$f"); }
[ -n "$token" ] || { echo "no token: set FORGEJO_TOKEN or put it in ~/.config/sprknotes/forgejo-token" >&2; exit 1; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
curl_api() { curl -sS -H "Authorization: token $token" "$@"; }       # body to stdout
pyj() { local c="$1"; shift; python3 -I -c "import json,sys; d=json.load(sys.stdin); $c" "$@"; }
code() { curl -sS -o "$tmp/out" -w '%{http_code}' -H "Authorization: token $token" "$@"; }

release_id() { # by tag, drafts included (the tag endpoint does not list drafts)
  curl_api "$api/releases?limit=50&draft=true" | pyj "
t=sys.argv[1]
for r in d:
    if r['tag_name']==t: print(r['id']); break" "$1" 2>/dev/null || true
}
need_id() { local id; id=$(release_id "$1"); [ -n "$id" ] || { echo "no release for $1; run: $0 draft $1" >&2; exit 1; }; echo "$id"; }
is_pre() { [[ "$1" == *-* ]] && echo true || echo false; }
upload_name() { # name Forgejo will accept
  case "$1" in *.zip|*.json|*.txt|*.gz|*.md) echo "$1" ;; *) echo "$1.zip" ;; esac
}

upload_one() { # id file
  local id="$1" f="$2" name want size c
  name=$(upload_name "$(basename "$f")"); size=$(stat -c %s "$f")
  # Replace an asset of the same name.
  curl_api "$api/releases/$id/assets" | pyj "
for a in d:
    if a['name']==sys.argv[1]: print(a['id'])" "$name" | while read -r aid; do
      code -X DELETE "$api/releases/$id/assets/$aid" >/dev/null; done
  c=$(code -F "attachment=@$f;filename=$name" "$api/releases/$id/assets?name=$name")
  if [ "$c" != 201 ]; then echo "FAIL $name (HTTP $c)"; head -c 300 "$tmp/out"; echo; return 1; fi
  got=$(pyj "print(d['size'])" < "$tmp/out")
  [ "$got" = "$size" ] || { echo "FAIL $name: stored $got bytes, sent $size"; return 1; }
  echo "ok   $name ($size bytes)"
}

case "$cmd" in
  draft)
    tag="${1:?tag}"; shift; notes=""; [ "${1:-}" = "--notes" ] && notes="$2"
    pre=$(is_pre "${tag#v}")
    body=$(python3 -I -c "import json,sys; print(json.dumps({'tag_name':sys.argv[1],'name':'SprkNotes '+sys.argv[1][1:],'body':open(sys.argv[2]).read() if sys.argv[2] else '','draft':True,'prerelease':sys.argv[3]=='true'}))" "$tag" "$notes" "$pre")
    id=$(release_id "$tag")
    if [ -n "$id" ]; then
      # Keep the published/draft state of an existing release; only refresh text and flag.
      body=$(python3 -I -c "import json,sys; d=json.loads(sys.argv[1]); d.pop('draft'); d.pop('tag_name'); print(json.dumps(d))" "$body")
      c=$(code -X PATCH -H 'Content-Type: application/json' -d "$body" "$api/releases/$id")
    else
      c=$(code -X POST -H 'Content-Type: application/json' -d "$body" "$api/releases")
      id=$(pyj "print(d['id'])" < "$tmp/out" 2>/dev/null || true)
    fi
    case "$c" in 200|201) echo "release $tag ready (id $id, pre-release=$pre)" ;; *) echo "FAIL draft $tag (HTTP $c)"; head -c 300 "$tmp/out"; exit 1 ;; esac ;;

  upload)
    tag="${1:?tag}"; shift; id=$(need_id "$tag")
    [ $# -gt 0 ] || { echo "no files" >&2; exit 2; }
    for f in "$@"; do upload_one "$id" "$f"; done ;;

  all)
    tag="${1:?tag}"; dir="${2:?dir}"; pub="${3:-}"
    id=$(need_id "$tag"); n=0
    while IFS= read -r f; do upload_one "$id" "$f"; n=$((n + 1)); done < <(find "$dir" -maxdepth 1 -type f | sort)
    [ "$n" -gt 0 ] || { echo "no files in $dir" >&2; exit 1; }
    [ "$pub" = "--publish" ] && exec "$0" publish "$tag"
    echo "$n files uploaded. Publish with: $0 publish $tag" ;;

  publish)
    tag="${1:?tag}"; id=$(need_id "$tag")
    names=$(curl_api "$api/releases/$id/assets" | pyj "print('\n'.join(a['name'] for a in d))")
    echo "$names" | grep -qx 'update-manifest.json' || { echo "refusing to publish: update-manifest.json is not attached (the app would not see this release)" >&2; exit 1; }
    echo "$names" | grep -qx 'SHA256SUMS.txt' || { echo "refusing to publish: SHA256SUMS.txt is not attached" >&2; exit 1; }
    c=$(code -X PATCH -H 'Content-Type: application/json' -d '{"draft":false}' "$api/releases/$id")
    [ "$c" = 200 ] && echo "published $tag: $host/$slug/releases/tag/$tag" || { echo "FAIL publish (HTTP $c)"; head -c 300 "$tmp/out"; exit 1; } ;;

  yank)
    tag="${1:?tag}"; id=$(need_id "$tag")
    c=$(code -X PATCH -H 'Content-Type: application/json' -d '{"draft":true}' "$api/releases/$id")
    [ "$c" = 200 ] && echo "$tag is a draft again: no longer offered to anyone (installed copies stay)" || { echo "FAIL (HTTP $c)"; exit 1; } ;;

  status)
    tag="${1:?tag}"; id=$(need_id "$tag")
    curl_api "$api/releases/$id" | pyj "
print('draft=%s prerelease=%s' % (d['draft'], d['prerelease']))
for a in d['assets']: print('  %10d  %s' % (a['size'], a['name']))" ;;

  *) sed -n '2,19p' "$0"; exit 2 ;;
esac
