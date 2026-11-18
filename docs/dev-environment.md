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
6. Toolchain on the guest: Visual Studio Build Tools (C++ workload, Windows 11 SDK), `rustup` (stable, MSVC), Node LTS, Git, PowerShell 7, WiX v5+ (`dotnet tool install --global wix`).
7. On the host, add to `~/.ssh/config`:
   ```
   Host sprk-winvm
     HostName <guest ip>
     User <guest user>
   ```
8. Verify: `scripts/winvm.sh check`, then `scripts/winvm.sh test`.

Use synthetic audio only. Never run real meetings through the VM.
