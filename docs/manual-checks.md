# Manual checks (only John can do these)

Everything here needs hardware, accounts, secrets or a product decision that the dev host and the VM do
not have. Work continues without them; tick items off and record results in the milestone notes.
Synthetic audio only, never real meetings. Do not put secrets, real names or tenant IDs in the repo.

Laptop bundle (M1): `~/sprknotes-work/m1-laptop-bundle/` on the dev host.

## Performance gates (SPEC 13.3), ARM64 Snapdragon and Intel x64
Unzip the exe zip for the machine plus `sprk-m1-models.zip` into one folder, then in PowerShell:
```
.\sprkctl.exe bench accuracy models\parakeet-tdt-0.6b-v2-int8 fx --vad models\silero-vad
.\sprkctl.exe bench accuracy models\whisper-base.en        fx --vad models\silero-vad
```
Record `RTF`, the `TOTAL` WER line, and peak Working Set (Task Manager > Details > sprkctl.exe).
Both streams roughly double decode load: compare `RTF x 2` with 0.3 (Snapdragon) or 0.5 (Intel).
- [ ] Snapdragon X: Parakeet and Whisper base.en (results go in the ADR-003 table)
- [ ] Intel: Parakeet and Whisper base.en
- [ ] Idle CPU about 0% when not transcribing (Task Manager)

## Per-milestone ARM64 check (SPEC 13.2a)
Copy the ARM64 portable build, run a 5-minute synthetic session (fixture played through speakers plus talking),
confirm transcript, labels, scrolling and the mini controller. Required for M2 (capture), M5, M6.
- [ ] M2 capture on Snapdragon
- [ ] M5 main UI on Snapdragon
- [ ] M6 mini controller and tray on Snapdragon

## Device tests (SPEC 13.2)
- [ ] Bleed test: laptop speakers on, fixture through system audio, user silent then talking; target at most 2% duplicate lines (M4)
- [ ] Device change test: plug and unplug a headset and Bluetooth mid-session (M2)
- [ ] Crash test on a real laptop: kill the process mid-session, recovery prompt works (M3)
- [ ] Scroll test: scroll up during a busy session for 60 s (M5)
- [ ] Offline test: networking disabled, every feature except AI handoff works the same
- [ ] Audio source test: Firefox and Edge, pause, resume, stop and switch rules (M11)
- [ ] Call context test: 1:1 Teams call, third person joins, Teams closed or API disabled (M12)

## Accounts and decisions
- [ ] **Confirm the default model** (ADR-003 is "proposed" until John says so directly): Parakeet TDT 0.6B v2 int8
- [ ] Classic Outlook test mailbox with a synthetic calendar (M8); new Outlook must show "unavailable" silently
- [ ] Teams: whether the local API is available on pilot devices (M12 spike)
- [ ] Claude and Microsoft 365 Copilot approved for PHI under company agreements before handoff buttons are enabled broadly (M9)
- [ ] Documents folder is OneDrive-redirected on target devices (SPEC 15)
- [ ] Bundling: ship the Parakeet pack inside the installer, ship the app small, or ship Whisper base.en (Parakeet is about 460 MiB compressed)
- [ ] Parakeet CC-BY-4.0 attribution wording approved (About screen, README, installer)

## Secrets and signing (never in the repo)
- [ ] Code-signing certificate source for the pilot (internal certificate via NinjaOne); set the thumbprint as an environment variable on the build VM (M13)
- [ ] Model signing key (M15, a 1.0 gate): create the Ed25519 keys in the dedicated 1Password vault, name the owner and backup holder, write the one-page key SOP
- [ ] Two signing keys registered (current and next) for rotation; public keys added to the signers file (M15)
- [ ] 1Password SSH agent enabled on the signing machine (`ssh-add -L` lists the key)

## Housekeeping
- [ ] Remove the temporary VM sudo rule from the dev host once the VM is set up
