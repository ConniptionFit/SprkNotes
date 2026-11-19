# Dev environment

## Dev host (CachyOS, x86_64)
```bash
sudo pacman -S --needed rustup clang lld nodejs npm git openssh
rustup default stable
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
cargo install cargo-xwin --locked
```
Check: `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check`
Cross-compile: `cargo xwin build --release --target x86_64-pc-windows-msvc` (and `aarch64-pc-windows-msvc`). The first run downloads the MSVC CRT and Windows SDK.

### Speech engine (M1)
The sherpa-onnx adapter is off by default. Fetch the native libs and models once (they go to `~/.cache/sprknotes`, never into the repo):
```bash
scripts/fetch-sherpa.sh            # linux-x64, win-x64 (static), win-arm64-shared
scripts/fetch-models.sh            # Parakeet, Whisper small/base, Silero VAD, as verified model folders
scripts/bench.sh --accuracy-only   # WER on LibriSpeech clips + synthetic speech
SHERPA_ONNX_LIB_DIR=$(scripts/fetch-sherpa.sh --print linux-x64) cargo run --release -p sprkctl --features engine,live -- \
  transcribe ~/.cache/sprknotes/models/parakeet-tdt-0.6b-v2-int8 some.wav --vad ~/.cache/sprknotes/models/silero-vad
```
Windows builds with the engine (run `scripts/xwin-case-links.sh` once after the first xwin build; ARM64 must use the shared libs, see ADR-003):
```bash
SHERPA_ONNX_LIB_DIR=$(scripts/fetch-sherpa.sh --print win-x64-shared)   cargo xwin build --release --target x86_64-pc-windows-msvc  -p sprkctl --features engine-shared,live
SHERPA_ONNX_LIB_DIR=$(scripts/fetch-sherpa.sh --print win-arm64-shared) cargo xwin build --release --target aarch64-pc-windows-msvc -p sprkctl --features engine-shared,live
```
The DLLs are copied next to `sprkctl.exe` in `target/<triple>/release/`. Needs `espeak-ng` and `ffmpeg` for the synthetic fixtures.

## Windows 11 x64 VM (`sprk-winvm`)
1. `sudo pacman -S --needed qemu-full libvirt virt-manager edk2-ovmf swtpm dnsmasq` then `sudo systemctl enable --now libvirtd` and add yourself to the `libvirt` group.
2. Create a Windows 11 guest in virt-manager: UEFI + TPM 2.0 (swtpm), 4+ vCPU, 8+ GB RAM, 80 GB virtio disk, virtio NIC. Attach the **virtio-win** ISO and install the storage driver during setup.
3. After install, run the virtio-win guest tools installer (network, balloon, audio, QXL/GPU).
4. Audio: use a PipeWire audio device (or SPICE audio) on the guest so WASAPI sees a render and capture device.
5. OpenSSH server (admin PowerShell):
   ```powershell
   Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
   Set-Service sshd -StartupType Automatic; Start-Service sshd
   ```
   Add your public key to `C:\ProgramData\ssh\administrators_authorized_keys`.
6. Toolchain on the guest: Visual Studio Build Tools (C++ workload, Windows 11 SDK), `rustup` (stable, MSVC), Node LTS, Git, PowerShell 7, WiX v5 (`dotnet tool install --global wix --version 5.*`; v7 and later need the OSMF EULA accepted by a person, so pin v5).
7. On the host, add to `~/.ssh/config`:
   ```
   Host sprk-winvm
     HostName <guest ip>
     User <guest user>
   ```
8. Verify: `scripts/winvm.sh check`, then `scripts/winvm.sh test`.

### Notes from the first VM build
- The guest's SSH default shell is PowerShell 5.1, so `scripts/winvm.sh` wraps everything in `powershell -Command` (no `&&`).
- `run-app` needs an interactive desktop: over plain SSH WebView2 fails with "Invalid window handle". Start it on the console session, for example with a scheduled task created with `/IT`.
- When attaching an unattended-install CD with `virsh attach-disk`, pass `--targetbus sata` (WinPE cannot read the default SCSI bus).

Use synthetic audio only. Never run real meetings through the VM.
