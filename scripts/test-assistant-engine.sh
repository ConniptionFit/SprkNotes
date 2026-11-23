#!/usr/bin/env bash
# Builds the llama.cpp helper and runs its tests against a tiny random-weight model (no download).
# Needs cmake, a C++ compiler, libclang (bindgen) and python3 with venv.
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install -q gguf numpy
"$work/venv/bin/python" scripts/make-test-assistant-model.py "$work/model"
cd crates/sprk-assistant-llama
cargo fmt --check
cargo clippy --all-targets -- -D warnings
SPRK_TEST_ASSISTANT_MODEL="$work/model" cargo test
