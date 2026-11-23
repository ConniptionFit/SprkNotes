#!/usr/bin/env bash
# The Forgejo and GitHub workflows must run the same jobs (see docs/forking.md).
set -euo pipefail
cd "$(dirname "$0")/.."
if ! diff <(sed -n '/^jobs:/,$p' .forgejo/workflows/ci.yml) <(sed -n '/^jobs:/,$p' .github/workflows/ci.yml) >/dev/null; then
  echo "check-ci-parity: .forgejo/workflows/ci.yml and .github/workflows/ci.yml jobs differ"; exit 1
fi
echo "check-ci-parity: ok"
