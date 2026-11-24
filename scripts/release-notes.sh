#!/usr/bin/env bash
# Usage: scripts/release-notes.sh <tag>      (markdown on stdout)
# What changed since the previous release: the CHANGELOG bullets added between the two tags, then
# the commit subjects. Stable notes cover everything since the previous stable release, so the
# people who skip the pre-releases still see all of it. Needs both tags (or the start of history).
set -euo pipefail
cd "$(dirname "$0")/.."
tag="${1:?usage: release-notes.sh <tag>}"; v="${tag#v}"
rv=scripts/release-version.sh
ch=$($rv channel "$v")
if [ "$ch" = stable ]; then prev=$($rv prev "$tag" --stable); else prev=$($rv prev "$tag"); fi
range=${prev:+$prev..}$tag

case "$ch" in
  alpha) echo "**Alpha test build.** For testing only; it may have rough edges. Expect a new alpha often." ;;
  beta)  echo "**Beta.** Feature-complete and being checked before it becomes stable." ;;
  *)     echo "**Stable release.**" ;;
esac
echo
echo "Changes since ${prev:-the start}:"
echo
bullets=$(git diff "${prev:-$(git hash-object -t tree /dev/null)}" "$tag" -- CHANGELOG.md | sed -n 's/^+\(- .*\)$/\1/p')
if [ -n "$bullets" ]; then echo "$bullets"; else
  git log --no-merges --pretty='- %s' "$range" | grep -vE '^- chore\(release\)' || echo "- No notable changes."
fi
echo
echo "<details><summary>Commits</summary>"
echo
git log --no-merges --pretty='- %s (%h)' "$range" | grep -vE '^- chore\(release\)' || true
echo
echo "</details>"
echo
echo "Unsigned Windows builds (pilot use). Check downloads against \`SHA256SUMS.txt\`. Installers carry a \`.zip\` suffix: unzip to get the file."
