# ADR-002: Dev environment

Status: accepted (M0)

## Context
Development happens on a CachyOS (x86_64) host. Windows behavior must still be tested for real. SPEC.md 3.4 and 13.4.

## Decision
- **Dev host (CachyOS):** edit, build and test all portable crates natively; cross-compile both Windows targets with `cargo-xwin`.
- **`sprk-winvm`:** Windows 11 x64 KVM guest reached over SSH for WASAPI, AEC, COM, Tauri UI and packaging tests. `scripts/winvm.sh` syncs the branch (git bundle) and runs commands there.
- **Snapdragon and Intel laptops:** only place performance gates (13.3) are measured and ARM64 ever runs. No ARM64 emulation, no Wine.
- **CI:** Linux, Windows x64 and Windows ARM64 jobs (`.github/workflows/ci.yml` and `windows.yml`).
- **Workspace split:** `app/src-tauri` is excluded from the root workspace so `cargo test --workspace` works on Linux without WebView2 or webkit libraries. Its version is mirrored from the root `Cargo.toml` by `scripts/bump-version.sh`, and `--check` runs in CI.

## Alternatives
- ARM64 Windows guest on x64 host: full CPU emulation, too slow for audio latency work.
- Wine: WebView2, WASAPI and COM differ from real Windows.
- Tauri as a normal workspace member: forces webkit2gtk onto every Linux build.

## Consequences
- Windows-only code can only be proven on the VM or Windows CI.
- Two Cargo manifests carry a version; the script and CI check keep them aligned.
- Note: SPEC.md M1 also says "ADR-002 picks the default model". That model ADR should be numbered ADR-003.
