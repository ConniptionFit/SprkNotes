# Manual checks (need a person, a laptop or a secret)

Everything here is something automated tests and the cloud/dev-host cannot do. Each item says
what it unblocks. Tick them off in the PR that carries the milestone.

## Laptops (SPEC 13.2a and 13.3)
- [ ] **M1 benchmarks, Snapdragon X (ARM64) and Intel (x64).** Use the laptop bundle (`sprk-m1-*.zip`) and run
  `.\sprkctl.exe bench accuracy models\parakeet-tdt-0.6b-v2-int8 fx --vad models\silero-vad`.
  Record `RTF`, `TOTAL` and peak Working Set in the ADR-003 table. Both streams roughly double decode
  load, so compare RTF x 2 with the gates (0.3 Snapdragon, 0.5 Intel). *Unblocks: ADR-003 final.*
- [ ] **Per-milestone ARM64 check** for milestones that touch audio, UI or packaging (M2, M5, M6, M13):
  copy the ARM64 portable build to the Snapdragon laptop, run a 5-minute synthetic session, confirm
  transcript, labels, scrolling and the mini controller. Note the result in the PR.

## Secrets and accounts (never in the repo)
- [ ] **Model signing keys (M15, 1.0 gate).** Create two Ed25519 SSH keys in 1Password (release owner and a
  separate backup holder, separate vaults). Export only the **public** keys into an `allowed_signers` file.
  *Unblocks: signing real packs and shipping 1.0.*
- [ ] **Interop check of `sprkctl model sign`.** With the 1Password SSH agent running:
  `sprkctl model sign dist\parakeet@2026.11.0.sprkmodel --pubkey signing.pub`, then
  `sprkctl model verify dist\parakeet@2026.11.0.sprkmodel --require-signature --keys allowed_signers`.
  Also confirm `ssh-keygen -Y verify -n sprknotes-model -f allowed_signers -I <principal> -s model.sig < model.json`
  accepts the same signature. (The verifier is tested with an in-process signer built to the SSHSIG
  format; real `ssh-keygen` output has not been checked.)
- [ ] **Code-signing certificate (M13).** Provide it to the Windows build host as an installed certificate or
  through your secret store. Do not put it in the repo or CI variables.

## Windows integrations (VM or real machine)
- [ ] **Classic Outlook test mailbox (M8).** A synthetic mailbox and calendar with Teams and Zoom meetings in the VM. No PHI.
- [ ] **Teams checks (M12).** 1:1 call, add a third person, drop back, Teams closed, local API disabled
  (SPEC 13.2 "Call context test").
- [ ] **Audio source test (M11).** Firefox and Edge: pause 20 s and resume, stop at 1:30 and wait 2:00,
  switch video, excluded app never appears (SPEC 13.2).
- [ ] **Bleed test (M4)** on the Snapdragon laptop with speakers on (SPEC 13.2).
- [ ] **Crash test (M3/M5):** kill the process mid-session, confirm the file holds every committed line
  and the recovery prompt works.
- [ ] **Device change test (M2):** plug and unplug a headset and a Bluetooth device mid-session.

## Batched VM run (when the RX9070 is free)
Once the Windows-only code lands, one pass in `sprk-winvm` covers it:
- [ ] `./scripts/winvm.sh test` (full workspace on Windows).
- [ ] `./scripts/winvm.sh run-app`: open the main window, confirm Start/Pause/End and the live pane scroll
  behavior (scroll up, "Return to live" pill, End key) once the Tauri commands are wired.
- [x] Windows installers and packaging (M13) through `release.ps1`: both MSIs have files, per-user install and uninstall leave nothing behind, version gate passes (2026-11-19, VM, throwaway cert).
- [ ] MSIX install: needs the signing cert in `LocalMachine\TrustedPeople` (admin, changes the VM trust store); run with a throwaway cert and remove it afterwards.
- [ ] Per-machine MSI (`ALLUSERS=1`) install and uninstall, and `Install-SprkNotes.ps1` as SYSTEM.
- [ ] arm64 MSI and portable build run on the Snapdragon laptop.
- [ ] Dev tools: WiX v5 (not v7, which needs the OSMF EULA accepted by a person), ARM64 MSVC component, Windows SDK tools.

## Windows capture and session (next VM run)
- [ ] `capture-check` with a real mic signal: route a host virtual source (for example a PipeWire null-sink monitor) into the VM's SPICE input. Loopback was verified; the VM mic read silence (no input signal available).
- [ ] Full Start to End flow in the app with the engine build (`--features engine-shared` plus `SHERPA_ONNX_LIB_DIR`), a bundled `models` folder next to the exe holding the transcription pack and the voice activity pack, and synthetic speech played on the VM's output. Confirm the `.md` and `.jsonl` appear under `Documents\SprkNotes\Transcripts`, lines appear live, Pause and Resume write events, End finalizes.
- [ ] Device change test (M2): switch the VM's default output mid-session and confirm a `Source changed` event.
- [ ] Bleed test on the Snapdragon laptop with speakers on (M4, ADR-004 filter length tuning).

## Decisions for John
- [ ] Parakeet needs a CC-BY-4.0 attribution notice: approve the wording for the About screen and README.
- [ ] Bundle Parakeet in the installer (about 460 MiB) or ship small and add the pack by RMM: chosen **bundle** (2026-11-19). Confirm installer size is acceptable for MSIX and Intune.
