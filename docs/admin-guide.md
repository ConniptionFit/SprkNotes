# SprkNotes admin guide

> [!IMPORTANT]
> **Deployment form: https://conniptionfit.github.io/SprkNotes/**. It builds the lock script (SYSTEM) and the per-user install script from the settings you pick, in your browser, with nothing sent anywhere. Walkthrough in [Deployment](deployment.md#admin-deployment-form).

## Artifacts (one version, built together)
| Artifact | Use |
|---|---|
| `SprkNotes_<ver>.msixbundle` | Intune LOB app (x64 + arm64), per-user |
| `SprkNotes_<ver>_{x64,arm64}.msi` | NinjaOne or GPO, per-user, or per-machine with `ALLUSERS=1` |
| `Install-SprkNotes.ps1` | One script for the fleet: picks the MSI for the OS architecture, `-Uninstall` supported |
| `SprkNotes_<ver>_portable.zip` | No-install use; delete the folder to remove |
| `sprkctl.exe` | Admin CLI, included in every artifact |
| `<id>@<ver>.sprkmodel` | Transcription model packs, versioned separately |
| `SHA256SUMS.txt`, `release-manifest.json` | Hashes and versions |

Linux is a from-source build with no packaged artifact yet (see [install.md](install.md#d-linux-native-build)); admin policy keys are Windows only. Uninstall never deletes transcripts. Install steps for one PC are in [install.md](install.md); fleet deployment (Intune, GPO, SCCM, RMM as SYSTEM) is in [deployment.md](deployment.md).

## Policy (registry)
`HKLM\Software\Policies\SprkNotes` wins over `HKCU\Software\Policies\SprkNotes`, which wins over user settings. A policy locks the matching setting.
Each key is its own value, or all of them packed in one string value named `Settings` (`Key=Value;Key=Value`, `|` between list items; what the MSI switch `POLICY` writes). Paths may use `%USERPROFILE%`, `%OneDrive%` and other variables, expanded for each user. To build the scripts without hand editing, use the [deployment form](https://conniptionfit.github.io/SprkNotes/) (also in this repository at `tools/admin-form/index.html`, which works offline). See [Deployment](deployment.md#admin-deployment-form).

The same keys can live in `%ProgramData%\SprkNotes\policy.json`: `{"settings": {"TranscriptsRoot": {"value": "%OneDrive%\\Transcripts", "locked": true}, "Theme": {"value": "dark", "locked": false}}}`. Order, setting by setting: machine registry, then the file, then user registry. The file counts only when its folder and itself are owned by Administrators or SYSTEM and nobody else can write them; otherwise SprkNotes ignores it and writes why to `%APPDATA%\SprkNotes\admin.log`. The form's lock script sets those permissions.

Window settings an admin can lock or start: `Theme` (system, light, dark), `ThrowbackMode`, `LiveCaptions`, `LiveLine`, `CaptionDelay` (fast, steady, smooth), `CaptionSpeakers` (others, both), `MicPrompt`, `Hotkey`, `HotkeyKeys` (for example `Ctrl+Alt+S`). Locked controls stay visible, greyed, with the hover text "This option is managed by your administrator." `WaveformStyle` is retired from 0.2.0-alpha.13 (the sound meter has one style now); a policy that still sets it is ignored, so there is nothing to clean up.

| Key | Meaning |
|---|---|
| `TranscriptsRoot` | Where transcripts go |
| `TranscriptRetentionDays` | Keep transcripts this many days (0 to 3650; 0 keeps them forever). Older ones go to the Recycle Bin, or are deleted when it can't take them, at start and once a day. An open or running transcript is never touched. Locks **Keep transcripts for** in Settings, which then says so; can also be a starting value |
| `CalendarProvider` | `com` or `off` |
| `ContextPrompt` | Ask / Always / Never add meeting details |
| `SummarizeEnabled` | 0 removes Summarize altogether (transcripts never go to Claude or Copilot) |
| `ShowClaudeHandoff`, `ShowCopilotHandoff` | Show or hide the buttons |
| `CopilotTarget`, `PromptTemplatePath` | Handoff target and prompt template |
| `FillerRemoval`, `CommitDelaySeconds` | Transcript behavior (delay 3 to 15 s) |
| `CustomWords` | Multi-string: names, medicines and product names to spell right, one per item. Adds to each person's own **Custom words** list and does not lock it. Taken as hints by a model pack that includes a `bpe_vocab` file, otherwise used to fix near misses after decoding (ADR-017). Never logged |
| `PinnedModel` | `id@version` forced as the active model |
| `AllowUserModels` | 0 or 1 |
| `AllowedModelHashes` | SHA-256 of `model.json`, pilot tier |
| `RequireSignedModels` | 0 or 1; always on from 1.0 |
| `ModelPublicKeys` | `allowed_signers` lines (Ed25519) |
| `KeepModelVersions` | Default 2 |
| `ModelDownload` | 0 turns the "Download model" button and all model network use off (default 1) |
| `ModelUpdateCheck` | 0 stops the launch-time check for a newer model (default 1) |
| `ModelIndexUrl` | https address of an internal mirror's `models-index.json` |
| `AssistantEnabled` | 0 or 1. The on-device assistant is off until turned on (ADR-009) |
| `AssistantTier` | Largest size allowed: `off`, `titles`, `light`, `standard` or `auto` |
| `AssistantPinnedModel` | `id@version` forced as the assistant model |
| `AssistantIdleUnloadMinutes`, `AssistantMaxThreads` | Unload delay (default 5) and a thread cap |
| `UpdateMode` | `off`, `notify`, `auto` or `force` (ADR-011). Locks the Settings control. See [deployment](deployment.md#updates-channels-and-locking) |
| `UpdateChannel` | `stable`, `beta` or `alpha`. Locks the channel |
| `UpdateRepoUrl` / `UpdateProvider` | https address of the repository releases come from, and `forgejo` or `github` when needed. Locks the source |
| `UpdatePublicKeys` | `ssh-ed25519` keys allowed to sign update manifests; unsigned updates are refused when set |
| `UpdateCheckHours` | Hours between update checks (default 24) |
| `AssistantFeatures` | Allow-list of assistant features, comma separated: `title,recap,actions,ask,agenda,commitments,pins,parkinglot,explain,shorten,prep,asked,summary,rolling`. Absent means the user chooses (the extras start off) |
| `AssistantSaveOutputs`, `AssistantBackground` | May it write notes next to the transcript; may it work in the background |
| `AudioSourceDetection`, `AudioMergeWindowSeconds`, `AudioSourceExcludePatterns` | Audio source events |
| `CallContext`, `CallPollSeconds`, `NameOneToOneSpeakers`, `CallMergeWindowSeconds`, `TeamsLocalApi` | Call context |

## Model runbook (`sprkctl`)
```powershell
sprkctl model verify  .\parakeet@2026.11.0.sprkmodel          # hashes, schema, (signature when required)
sprkctl model install .\parakeet@2026.11.0.sprkmodel --scope machine   # elevated / SYSTEM
sprkctl model list --json
sprkctl model activate <id>@<ver>
sprkctl model rollback
# Assistant packs (off by default, ADR-009) live in their own folder; add --kind assistant:
sprkctl model install .\qwen3-5-2b-q4@2027.01.0.sprkmodel --kind assistant --scope machine
sprkctl model list --kind assistant --scope machine --json
sprkctl model remove <id>@<ver>
```
- A new or rolled-back model takes effect at the **next session start**, never mid-session.
- A model that fails to load falls back to the previous good model.
- Packs contain data only and never run code. Extraction blocks path traversal and size bombs.
- Exit codes are RMM-friendly (0 success, non-zero with a reason on stderr).

### Signing (required for 1.0)
Packs are signed with Ed25519 (OpenSSH SSHSIG, namespace `sprknotes-model`) through the 1Password SSH agent: `sprkctl model sign <pack> --pubkey <file>`. Two public keys are accepted so keys can rotate. Private keys never touch the repo or endpoints. Pilot builds may use `AllowedModelHashes` instead; 1.0 cannot ship without signature enforcement.

## Privacy and network
SprkNotes makes no outbound connections except model downloads (ADR-005): at launch it fetches a small model index to see whether a newer model exists, and downloads a pack only when the user presses "Download model". Nothing about the user or any transcript is sent. Set `ModelDownload=0` to turn all of it off, `ModelUpdateCheck=0` to stop only the launch check, or `ModelIndexUrl` to point at an internal mirror. Otherwise expect only `localhost` traffic (Teams local API, if enabled). `sprkctl model sync` (admin-run) reads an internal file share. The app also checks the release page for a newer SprkNotes (ADR-011): plain GETs for the release list, a small signed manifest and, when an update is due, the installer. `UpdateMode=off` stops all of it; `UpdateRepoUrl` points it at your own source. No telemetry or crash upload. Logs hold IDs and counts only; context data (titles, names) goes only into transcript files.

## Live captions
Captions and the live line are shown from in-memory text only: never written to disk, never logged, never part of the transcript (ADR-006, ADR-007, ADR-008). They are a per-user setting in the app; there is no policy key. Others-only is the default and the cheapest on CPU. If decoding cannot keep up, the engine switches captions off for the session.

## Troubleshooting
- **No model available:** the bundled default is read-only in the app folder; check `sprkctl model list`.
- **Calendar names missing:** classic Outlook with a profile is required. New Outlook is reported unavailable silently.
- **Teams names missing:** the Teams local API may be off; the title falls back to the call window title or `Others`.
- **Interrupted transcript:** the file is complete up to the last committed line. At the next launch the user is offered Open and Mark finished (closes it as interrupted).
- **"A line could not be saved" notice:** the transcripts folder is not writable or the disk is full; check `TranscriptsRoot` and free space.

## Publishing a model update (maintainers)
The app's default index is `models-index.json` on the repository's `models` release. To publish a new model:
```powershell
sprkctl model pack .\models-src\<id> -o .\dist\<id>@<ver>.sprkmodel
# the host only accepts some file types, so upload each pack with a .zip suffix and say so in the index
sprkctl model index .\dist\parakeet....sprkmodel .\dist\silero-vad....sprkmodel `
  --base-url https://<host>/<owner>/<repo>/releases/download/models --url-suffix .zip -o models-index.json
```
Hosts that cap upload size (the maintainers' host rejects bodies over about 100 MB) need big packs split: add `--split-mb 80 --parts-dir .\dist\parts` and the index lists each part with its own checksum (the app joins them and checks the whole-pack checksum before import). Upload the files in `parts` instead of the pack. Otherwise upload `models-index.json` and each pack (renamed `<file>.zip`) to the `models` release, replacing older assets. The same files work on an internal mirror: change `--base-url`. From 1.0 packs must also be signed (`sprkctl model sign`).

## Building a release (maintainers)
On the Windows VM, from the repo root:
```powershell
pwsh scripts/release.ps1 -SherpaX64 <dir> -SherpaArm64 <dir> -ModelsDir <store> -CertThumbprint <sha1>
```
It builds both architectures, the MSIs, the MSIX bundle, the portable zip and the install scripts, then
fails the release if any artifact reports a different version than the root `Cargo.toml`. Without a
certificate thumbprint the output is marked unsigned and is for development only.
