#!/usr/bin/env bash
# GitHub half of publish-release.sh (same commands). Uses the gh CLI: GH_TOKEN in CI, or `gh auth login`.
# GitHub takes any file name up to 2 GB, so there is no .zip suffix trick here.
set -euo pipefail
cd "$(dirname "$0")/.."
cmd="${1:-}"; shift || true
repo_url="${SPRK_REPO_URL:-$(sed -n 's/^repo_url *= *"\(.*\)"/\1/p' distribution.toml)}"
slug="${repo_url#https://github.com/}"; slug="${slug%.git}"
gh_() { gh "$@" --repo "$slug"; }
is_pre() { [[ "$1" == *-* ]] && echo --prerelease || true; }
exists() { gh release view "$1" --repo "$slug" >/dev/null 2>&1; }

case "$cmd" in
  draft)
    tag="${1:?tag}"; shift; notes=""; [ "${1:-}" = "--notes" ] && notes="$2"
    pre=$(is_pre "${tag#v}")
    nf=(); [ -n "$notes" ] && nf=(--notes-file "$notes") || nf=(--notes "")
    if exists "$tag"; then gh release edit "$tag" --repo "$slug" --title "SprkNotes ${tag#v}" "${nf[@]}" ${pre:+--prerelease}
    else gh release create "$tag" --repo "$slug" --draft --verify-tag --title "SprkNotes ${tag#v}" "${nf[@]}" ${pre:+--prerelease}; fi
    echo "release $tag ready" ;;
  upload)
    tag="${1:?tag}"; shift; [ $# -gt 0 ] || { echo "no files" >&2; exit 2; }
    gh release upload "$tag" --repo "$slug" --clobber "$@" ;;
  all)
    tag="${1:?tag}"; dir="${2:?dir}"; pub="${3:-}"
    mapfile -t files < <(find "$dir" -maxdepth 1 -type f | sort)
    [ ${#files[@]} -gt 0 ] || { echo "no files in $dir" >&2; exit 1; }
    gh release upload "$tag" --repo "$slug" --clobber "${files[@]}"
    [ "$pub" = "--publish" ] && exec "$0" publish "$tag"
    echo "${#files[@]} files uploaded. Publish with: $0 publish $tag" ;;
  publish)
    tag="${1:?tag}"
    names=$(gh release view "$tag" --repo "$slug" --json assets --jq '.assets[].name')
    grep -qx 'update-manifest.json' <<<"$names" || { echo "refusing to publish: update-manifest.json is not attached (the app would not see this release)" >&2; exit 1; }
    grep -qx 'SHA256SUMS.txt' <<<"$names" || { echo "refusing to publish: SHA256SUMS.txt is not attached" >&2; exit 1; }
    gh release edit "$tag" --repo "$slug" --draft=false
    echo "published $tag: $repo_url/releases/tag/$tag" ;;
  yank)
    tag="${1:?tag}"; gh release edit "$tag" --repo "$slug" --draft=true
    echo "$tag is a draft again: no longer offered to anyone (installed copies stay)" ;;
  status)
    tag="${1:?tag}"
    gh release view "$tag" --repo "$slug" --json isDraft,isPrerelease,assets --jq '"draft=\(.isDraft) prerelease=\(.isPrerelease)", (.assets[] | "  \(.size)  \(.name)")' ;;
  *) sed -n '2,3p' "$0"; exit 2 ;;
esac
