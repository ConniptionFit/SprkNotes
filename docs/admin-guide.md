# SprkNotes admin guide

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

Uninstall never deletes transcripts. Packaging status: see CHANGELOG (M13 pending).

## Policy (registry)
`HKLM\Software\Policies\SprkNotes` wins over `HKCU\Software\Policies\SprkNotes`, which wins over user settings. A policy locks the matching setting.

| Key | Meaning |
|---|---|
| `TranscriptsRoot` | Where transcripts go |
| `CalendarProvider` | `com` or `off` |
| `ContextPrompt` | Ask / Always / Never add meeting details |
| `ShowClaudeHandoff`, `ShowCopilotHandoff` | Show or hide the buttons |
| `CopilotTarget`, `PromptTemplatePath` | Handoff target and prompt template |
| `FillerRemoval`, `CommitDelaySeconds` | Transcript behavior (delay 3 to 15 s) |
| `PinnedModel` | `id@version` forced as the active model |
| `AllowUserModels` | 0 or 1 |
| `AllowedModelHashes` | SHA-256 of `model.json`, pilot tier |
| `RequireSignedModels` | 0 or 1; always on from 1.0 |
| `ModelPublicKeys` | `allowed_signers` lines (Ed25519) |
| `KeepModelVersions` | Default 2 |
| `AudioSourceDetection`, `AudioMergeWindowSeconds`, `AudioSourceExcludePatterns` | Audio source events |
| `CallContext`, `CallPollSeconds`, `NameOneToOneSpeakers`, `CallMergeWindowSeconds`, `TeamsLocalApi` | Call context |

## Model runbook (`sprkctl`)
```powershell
sprkctl model verify  .\parakeet@2026.11.0.sprkmodel          # hashes, schema, (signature when required)
sprkctl model install .\parakeet@2026.11.0.sprkmodel --scope machine   # elevated / SYSTEM
sprkctl model list --json
sprkctl model activate <id>@<ver>
sprkctl model rollback
sprkctl model remove <id>@<ver>
```
- A new or rolled-back model takes effect at the **next session start**, never mid-session.
- A model that fails to load falls back to the previous good model.
- Packs contain data only and never run code. Extraction blocks path traversal and size bombs.
- Exit codes are RMM-friendly (0 success, non-zero with a reason on stderr).

### Signing (required for 1.0)
Packs are signed with Ed25519 (OpenSSH SSHSIG, namespace `sprknotes-model`) through the 1Password SSH agent: `sprkctl model sign <pack> --pubkey <file>`. Two public keys are accepted so keys can rotate. Private keys never touch the repo or endpoints. Pilot builds may use `AllowedModelHashes` instead; 1.0 cannot ship without signature enforcement.

## Privacy and network
SprkNotes makes no outbound connections. Expect only `localhost` traffic (Teams local API, if enabled). `sprkctl model sync` (admin-run) reads an internal file share. No telemetry, update checks or crash upload. Logs hold IDs and counts only; context data (titles, names) goes only into transcript files.

## Troubleshooting
- **No model available:** the bundled default is read-only in the app folder; check `sprkctl model list`.
- **Calendar names missing:** classic Outlook with a profile is required. New Outlook is reported unavailable silently.
- **Teams names missing:** the Teams local API may be off; the title falls back to the call window title or `Others`.
- **Interrupted transcript:** the file is complete up to the last committed line.
