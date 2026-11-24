#!/usr/bin/env bash
# Release version rules in one place (docs/releasing.md). Used by cut-release.sh, the release
# workflow and by hand.
#
#   release-version.sh next <alpha|beta|stable> [--base X.Y.Z] [--from TAG]   print the next version
#   release-version.sh verify-tag <TAG>     tag matches the workspace version (run on the tagged commit)
#   release-version.sh guard <TAG>          a beta/stable tag differs from the build it promotes only
#                                           in version files and CHANGELOG.md
#   release-version.sh prev <TAG> [--stable] the release tag just before TAG (or just before it among stable tags)
#   release-version.sh channel <VERSION>    alpha | beta | stable
#   release-version.sh prerelease <VERSION> true | false
#
# Versions are SemVer: X.Y.Z (stable), X.Y.Z-beta.N, X.Y.Z-alpha.N. Tags are v<version>.
set -euo pipefail
cd "${SPRK_ROOT:-$(dirname "$0")/..}"

RE_VER='^[0-9]+\.[0-9]+\.[0-9]+(-(alpha|beta)\.[0-9]+)?$'
die() { echo "release-version: $*" >&2; exit 1; }
base_of() { echo "${1%%-*}"; }
channel_of() { case "$1" in *-alpha.*) echo alpha ;; *-beta.*) echo beta ;; *) echo stable ;; esac; }
tags() { git tag -l 'v[0-9]*' | sed 's/^v//' | grep -E "$RE_VER" || true; }
vsort() { sort -t. -k1,1n -k2,2n -k3,3n; }
workspace_version() { sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml; }
# Highest N for tags of one base and one channel (0 when none).
max_n() { tags | sed -n "s/^$(echo "$1" | sed 's/\./\\./g')-$2\.\([0-9]*\)$/\1/p" | sort -n | tail -1 | grep . || echo 0; }

cmd="${1:-}"; shift || true
case "$cmd" in
  channel) channel_of "${1:?version}" ;;
  prerelease) [ "$(channel_of "${1:?version}")" = stable ] && echo false || echo true ;;

  next)
    kind="${1:?alpha|beta|stable}"; shift
    base=""; from=""
    while [ $# -gt 0 ]; do
      case "$1" in --base) base="$2"; shift 2 ;; --from) from="$2"; shift 2 ;; *) die "unknown option $1" ;; esac
    done
    [[ -z "$base" || "$base" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "--base must be X.Y.Z"
    stables=$(tags | grep -vE -- '-' | vsort || true)
    case "$kind" in
      alpha)
        if [ -z "$base" ]; then
          latest=$(echo "$stables" | tail -1)
          # Continue the newest unfinished base (an alpha or beta whose stable is not out yet).
          open=""
          for b in $(tags | sed -n 's/-.*//p' | sort -u | vsort); do
            echo "$stables" | grep -qx "$b" || open="$b"
          done
          if [ -n "$open" ]; then base="$open"
          elif [ -n "$latest" ]; then IFS=. read -r a b c <<<"$latest"; base="$a.$b.$((c + 1))"
          else base="0.1.0"; fi
        fi
        echo "$base-alpha.$(( $(max_n "$base" alpha) + 1 ))" ;;
      beta|stable)
        if [ -z "$base" ]; then
          [ -n "$from" ] || from=$(git tag -l 'v*-alpha.*' 'v*-beta.*' | sed 's/^v//' | vsort | tail -1)
          [ -n "$from" ] || die "no earlier alpha to promote; pass --from TAG or --base X.Y.Z"
          base=$(base_of "${from#v}")
        fi
        if [ "$kind" = beta ]; then echo "$base-beta.$(( $(max_n "$base" beta) + 1 ))"
        else
          echo "$stables" | grep -qx "$base" && die "v$base already exists"
          echo "$base"
        fi ;;
      *) die "kind must be alpha, beta or stable" ;;
    esac ;;

  prev)
    tag="${1:?tag}"; shift; only_stable=0; [ "${1:-}" = "--stable" ] && only_stable=1
    # Sort key by SemVer precedence: for one X.Y.Z, alpha < beta < stable.
    keyed() { awk -F'[.-]' '{ s = ($0 ~ /-alpha\./) ? 1 : ($0 ~ /-beta\./) ? 2 : 3
      printf "%06d%06d%06d%d%06d %s\n", $1, $2, $3, s, (s < 3 ? $5 : 0), $0 }'; }
    mine=$(echo "${tag#v}" | keyed | cut -d' ' -f1)
    list=$(tags); [ "$only_stable" = 1 ] && list=$(echo "$list" | { grep -v -- '-' || true; })
    echo "$list" | keyed | sort | awk -v k="$mine" '$1 < k { last = $2 } END { if (last) print "v" last }' ;;

  verify-tag)
    tag="${1:?tag}"; v="${tag#v}"
    [[ "$v" =~ $RE_VER ]] || die "tag '$tag' is not vX.Y.Z, vX.Y.Z-beta.N or vX.Y.Z-alpha.N"
    [ "$(workspace_version)" = "$v" ] || die "Cargo.toml says $(workspace_version) but the tag says $v"
    scripts/bump-version.sh --check ;;

  guard)
    tag="${1:?tag}"; v="${tag#v}"; ch=$(channel_of "$v"); b=$(base_of "$v")
    [ "$ch" = alpha ] && { echo "guard: alpha builds come straight from main, nothing to compare"; exit 0; }
    # The build being promoted: the newest earlier pre-release of the same base.
    case "$ch" in beta) pat='alpha' ;; stable) pat='alpha beta' ;; esac
    src=""
    for p in $pat; do
      c=$(git tag -l "v$b-$p.*" | sed 's/^v//' | sort -t. -k4,4n | tail -1)
      [ -n "$c" ] && { [ "$p" = beta ] && { src="v$c"; break; }; [ -z "$src" ] && src="v$c"; }
    done
    if [ -z "$src" ]; then echo "guard: no earlier pre-release of $b (a hotfix?), skipping"; exit 0; fi
    allowed='^(Cargo\.toml|Cargo\.lock|CHANGELOG\.md|app/src-tauri/Cargo\.(toml|lock)|app/ui/package(-lock)?\.json)$'
    bad=$(git diff --name-only "$src" "$tag" | grep -vE "$allowed" || true)
    if [ -n "$bad" ]; then
      echo "guard: $tag is not the tested build $src plus a version bump. Changed beyond version files:" >&2
      echo "$bad" >&2; exit 1
    fi
    echo "guard: $tag = $src + version files only" ;;

  *) sed -n '2,12p' "$0"; exit 2 ;;
esac
