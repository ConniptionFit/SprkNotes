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

## App updates and install (ADR-011), on the Windows VM
- [ ] **One-line install.** In a normal PowerShell on a clean user: run the `irm ... install.ps1 | iex` line against a real release that carries `update-manifest.json`. Confirm the Start menu entry, `%LocalAppData%\Programs\SprkNotes`, then `install.ps1 -Uninstall`. (Written without a PowerShell to run it in; not yet exercised.)
- [ ] **MSI switches.** `msiexec /i SprkNotes_<ver>_x64.msi ALLUSERS=1 /qn UPDATE_CHANNEL=beta UPDATE_MODE=auto LOCK_UPDATES=1`, then check `HKLM\Software\Policies\SprkNotes` holds the values and Settings shows the channel as managed. Repeat without `LOCK_UPDATES` and check `HKLM\Software\SprkNotes\Defaults` and an editable channel.
- [ ] **Update task.** After a per-machine install, `schtasks /Query /TN "SprkNotes\Update" /V` shows SYSTEM and a 6 hour repeat. Publish a newer test release, run `schtasks /Run /TN "SprkNotes\Update"` with the app closed (version changes) and with the app open (nothing happens, installer waits). Uninstall removes the task.
- [ ] **Per-user update.** Install an older test build per user, publish a newer one, Settings, Check for updates, "Update when I close", close: a Windows Installer progress window shows, and the new version runs at next launch.
- [ ] **Update now.** Same setup, Settings, Check for updates, **Update now**: the download bar moves, SprkNotes closes, the progress window shows, and SprkNotes opens again on the new version by itself. During a transcript, Update now is refused with "End the transcript first".
- [ ] **Signing.** `scripts/release.ps1 -UpdateSigningKey <pub>` through the 1Password agent; confirm `ssh-keygen -Y verify -n sprknotes-update` accepts the signature and the app refuses a tampered manifest.
- [ ] **ADMX draft** loads in Intune (custom ADMX ingestion) or `gpedit` and writes the same registry values.

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
- [x] MSIX x64 install with a throwaway cert in `LocalMachine\TrustedPeople` (needs an interactive elevated session, SSH logons get Access denied); registered, sprkctl present, removed cleanly (2026-11-19). arm64 msix, bundle and launching the app are untested.
- [x] Per-machine MSI and `Install-SprkNotes.ps1 -Machine` / `-Uninstall` (elevated, VM): clean (2026-11-19). Not yet run as SYSTEM through the RMM.
- [ ] arm64 MSI and portable build run on the Snapdragon laptop.
- [ ] Dev tools: WiX v5 (not v7, which needs the OSMF EULA accepted by a person), ARM64 MSVC component, Windows SDK tools.

## Windows capture and session (next VM run)
- [ ] `capture-check` with a real mic signal: route a host virtual source (for example a PipeWire null-sink monitor) into the VM's SPICE input. Loopback was verified; the VM mic read silence (no input signal available).
- [ ] Full Start to End flow in the app with the engine build (`--features engine-shared` plus `SHERPA_ONNX_LIB_DIR`), a bundled `models` folder next to the exe holding the transcription pack and the voice activity pack, and synthetic speech played on the VM's output. Confirm the `.md` and `.jsonl` appear under `Documents\SprkNotes\Transcripts`, lines appear live, Pause and Resume write events, End finalizes.
- [ ] Device change test (M2): switch the VM's default output mid-session and confirm a `Source changed` event.
- [ ] Bleed test on the Snapdragon laptop with speakers on (M4, ADR-013): play a video through the speakers while silent, then while talking; expect no Me lines from the video and your own speech kept. Round 4: also pause and restart the video every few seconds for two minutes, and play short clips (a few words each) with gaps; those were the cases that leaked before. Check CPU stays within the 13.3 gate.

## Library organize (SPEC 8.4), on the Windows VM
- [ ] Create a folder and a subfolder, move a transcript in, rename the folder, then **View in folder** opens the new place. Rename a transcript and confirm the file name and the `.sprknotes` sidecar follow.
- [ ] **Delete**: the transcript and its sidecar land in the Recycle Bin and **Restore** brings them back; a summary next to it stays. With a session running, rename, move and delete of its transcript are refused with "End the current session first."

## Decisions for John
- [ ] Parakeet needs a CC-BY-4.0 attribution notice: approve the wording for the About screen and README.
- [ ] Bundle Parakeet in the installer (about 460 MiB) or ship small and add the pack by RMM: chosen **bundle** (2026-11-19). Confirm installer size is acceptable for MSIX and Intune.
