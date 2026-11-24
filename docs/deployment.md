# Deployment guides

For one PC see [install.md](install.md). This page is for fleets. Every method installs the same version; use `scripts/release.ps1` output so the MSIs, MSIX bundle and portable zip always match. Verified paths are marked; everything else is documented but not yet exercised on a managed fleet.

## What to push
| Goal | Artifact | Verified |
|---|---|---|
| Intune, per-user, one assignment for x64 and ARM64 | `SprkNotes_<ver>.msixbundle` | Installs and removes on a test VM (x64) |
| Intune Win32 app, NinjaOne, any RMM | MSIs plus `Install-SprkNotes.ps1` | Script installs and removes per-machine (elevated) |
| GPO or SCCM | The MSI for each architecture | MSI install and uninstall verified by hand |
| No install | Portable zip | Runs on a test VM |
| Models | `.sprkmodel` plus `Install-SprkModel.ps1` | Script parses; installs through `sprkctl` |

## Code signing
- **MSIX** will not install unless the signing certificate chains to a root the device trusts. Use a code-signing certificate from a trusted CA, or distribute your internal CA or the public `.cer` to **Trusted People** (Intune: Trusted certificate profile; GPO: Computer Configuration, Policies, Windows Settings, Security Settings, Public Key Policies, Trusted People).
- **MSI and exe** work unsigned, but Windows shows SmartScreen warnings and managed devices with application control may block them. Sign with the same certificate when you have one (`scripts/release.ps1 -CertThumbprint <thumbprint>`).
- Never put the certificate or its private key in the repo or in CI variables.

## Intune
**Option 1: MSIX bundle (Line-of-business app).** Upload `SprkNotes_<ver>.msixbundle`, assign to devices or users. Per-user, no admin, one file covers x64 and ARM64. Deploy the signing certificate as a Trusted certificate profile (Trusted People) first.

**Option 2: Win32 app (`.intunewin`).** `release.ps1` wraps the MSIs and the install script when `IntuneWinAppUtil.exe` is on the PATH (not exercised yet).
- Install: `powershell.exe -ExecutionPolicy Bypass -File Install-SprkNotes.ps1 -Machine`
- Uninstall: `powershell.exe -ExecutionPolicy Bypass -File Install-SprkNotes.ps1 -Uninstall`
- Install behavior: System
- Detection rule: file exists `%ProgramFiles%\SprkNotes\SprkNotes.exe`
- Requirement: 64-bit OS. The script picks x64 or ARM64 itself.

Deploy models as a second Win32 app (below) and registry policy through a Settings catalog or PowerShell script (below).

## Group Policy and SCCM
Neither is exercised yet; these follow standard Windows Installer behavior.
- **GPO software installation:** put `SprkNotes_<ver>_x64.msi` on a share readable by computer accounts and assign it to computers (Computer Configuration, Software Settings, Software installation). Use a separate GPO filtered to ARM64 devices for the arm64 MSI.
- **GPO startup script alternative:** run `Install-SprkNotes.ps1 -Machine` from a startup script; it picks the right MSI and logs to `%ProgramData%\SprkNotes\install.log`.
- **SCCM / Configuration Manager:** create a Windows Installer (MSI) application per architecture and let the product code from the MSI drive detection, with requirement rules for OS architecture. Or deploy `Install-SprkNotes.ps1 -Machine` as a script application with the file detection rule above. Install for system.
- **Upgrade:** installing a newer MSI replaces the old one (major upgrade). Downgrades are refused.

## RMM as SYSTEM (NinjaOne and similar)
Put the MSIs, `Install-SprkNotes.ps1` and, for models, `sprkctl.exe`, `Install-SprkModel.ps1` and the pack in one folder on the endpoint (or a share it can read), then run as SYSTEM:
```powershell
powershell.exe -ExecutionPolicy Bypass -File .\Install-SprkNotes.ps1 -Machine
powershell.exe -ExecutionPolicy Bypass -File .\Install-SprkModel.ps1 -Pack .\<id>@<version>.sprkmodel
```
Exit codes: the app script returns the `msiexec` exit code (0 success, 3010 reboot requested). `Install-SprkModel.ps1` returns `sprkctl` codes (10 verify failed, 12 incompatible, 20 not elevated). Uninstall: `Install-SprkNotes.ps1 -Uninstall`. Everything logs under `%ProgramData%\SprkNotes`.

## Models at scale
- **Download model button (default on).** The light build gets its model from the Settings pane and checks for a newer one at launch (details in ADR-005). Allow https to the model host in your firewall, or run an internal mirror: publish `models-index.json` and the packs on an internal web server and set `ModelIndexUrl` to its https address. `ModelDownload=0` turns all model network use off; `ModelUpdateCheck=0` stops only the launch check. Use a `with-model` build or `Install-SprkModel.ps1` on locked-down PCs.
- One default model can be bundled with the app, or shipped as a pack with `Install-SprkModel.ps1` (machine scope).
- A new model activates at the next session start. A failed load falls back to the previous good model.
- To force a model, set `PinnedModel` to `id@version`. To stop users adding their own, set `AllowUserModels` to `0`.
- Pilot builds can restrict packs by `AllowedModelHashes`. From 1.0 every pack must carry a valid signature from a key listed in `ModelPublicKeys`.

## Registry policy
Policy under `HKLM\Software\Policies\SprkNotes` (machine) wins over `HKCU\Software\Policies\SprkNotes` (user), which wins over user settings. The full key list is in the [admin guide](admin-guide.md#policy-registry). There is no ADMX template yet, so set values with a script, a `.reg` file, Group Policy Preferences (Registry) or an Intune Settings catalog custom entry.

Example `.reg`:
```
Windows Registry Editor Version 5.00

[HKEY_LOCAL_MACHINE\Software\Policies\SprkNotes]
"TranscriptsRoot"="D:\\Transcripts"
"AudioSourceDetection"=dword:00000001
"CallContext"=dword:00000001
"NameOneToOneSpeakers"=dword:00000000
"CallPollSeconds"=dword:0000000f
"AudioSourceExcludePatterns"=hex(7):62,00,61,00,6e,00,6b,00,69,00,6e,00,67,00,00,00,00,00,00,00
```
Example PowerShell (as SYSTEM):
```powershell
$k = 'HKLM:\Software\Policies\SprkNotes'
New-Item -Path $k -Force | Out-Null
New-ItemProperty $k -Name TranscriptsRoot -Value 'D:\Transcripts' -PropertyType String -Force | Out-Null
New-ItemProperty $k -Name NameOneToOneSpeakers -Value 0 -PropertyType DWord -Force | Out-Null
New-ItemProperty $k -Name AudioSourceExcludePatterns -Value @('banking','vault') -PropertyType MultiString -Force | Out-Null
```
Policy is read when a session starts or a download is requested. Values it cannot parse are ignored and the default applies. Registry policy for the transcripts root, commit delay, filler removal, audio sources and call context is wired in; other keys are read as the matching features land.

## Verify a deployment
```powershell
(Get-Item "$env:ProgramFiles\SprkNotes\SprkNotes.exe").VersionInfo.ProductVersion
& "$env:ProgramFiles\SprkNotes\sprkctl.exe" --version
& "$env:ProgramFiles\SprkNotes\sprkctl.exe" model list --json
```
Compare the version with `release-manifest.json` and check file hashes against `SHA256SUMS.txt`.

## Remove
MSI or script uninstall as above; MSIX via `Remove-AppxPackage`; portable by deleting the folder. Transcripts are never deleted by uninstalling. Policy keys stay until you remove them.

## Updates, channels and locking
SprkNotes updates from the releases of a repository (Forgejo by default, GitHub supported). Design and options: [ADR-011](adr/011-app-updates.md) and [enterprise-install-research.md](enterprise-install-research.md).

**How a release is found.** Tags `vX.Y.Z` are stable, `vX.Y.Z-beta.N` beta, `vX.Y.Z-alpha.N` alpha. A release must carry `update-manifest.json` (and `update-manifest.json.sig` when signed). A channel includes the more stable ones, and the app never downgrades.

### Policy keys (`HKLM\Software\Policies\SprkNotes`, or the user hive)
| Key | Type | Meaning |
|---|---|---|
| `UpdateMode` | string | `off` (no update traffic), `notify` (default for one user), `auto` (install quietly when the app is not in use), `force` (like auto; users cannot change it) |
| `UpdateChannel` | string | `stable`, `beta` or `alpha`. Locks the Settings control |
| `UpdateRepoUrl` | string | https address of the repository (for example `https://git.pwrs.dev/powers/sprknotes`). Locks the control |
| `UpdateProvider` | string | `forgejo` or `github`; only needed when the address does not make it obvious (GitHub Enterprise) |
| `UpdatePublicKeys` | string or multi-string | OpenSSH `ssh-ed25519 ...` keys allowed to sign update manifests. When set, unsigned updates are refused |
| `UpdateCheckHours` | dword | Hours between checks (default 24) |

A value set by policy wins and its control shows as managed. Order: policy, the user's choice, installer defaults, built-in defaults (stable, notify, the SprkNotes repository). An ADMX draft is in `packaging/admx` (not yet tried in Intune or gpedit).

### Install switches (MSI)
```powershell
msiexec /i SprkNotes_<ver>_x64.msi ALLUSERS=1 /qn /norestart `
  UPDATE_CHANNEL=beta UPDATE_REPO_URL="https://git.pwrs.dev/powers/sprknotes" UPDATE_MODE=auto LOCK_UPDATES=1
```
| Switch | Effect |
|---|---|
| `UPDATE_CHANNEL`, `UPDATE_REPO_URL`, `UPDATE_PROVIDER`, `UPDATE_MODE`, `UPDATE_PUBLIC_KEY` | Preconfigure the setting |
| `LOCK_UPDATES=1` | Write them as policy (locked in Settings). Without it they are starting values the user may change. A real lock needs a per-machine install (HKLM); a per-user install writes the user's own hive |
| `UPDATE_TASK=0` | Per-machine only: do not create the update task (use when Intune or an RMM pushes updates) |

`Install-SprkNotes.ps1` and `install.ps1` take the same options as parameters.

### Per-machine installs update themselves
A per-machine MSI registers the scheduled task `SprkNotes\Update` (runs as SYSTEM every 6 hours, no window). It runs `SprkNotes.exe --update`: check on the `UpdateCheckHours` interval, download, verify the SHA-256 and signature, and install only when nobody has SprkNotes open. If it is open, the verified installer waits for the next run. Per-machine installs start in `auto` mode; set `UpdateMode=off` or `UPDATE_TASK=0` to manage versions yourself. Check it: `schtasks /Query /TN "SprkNotes\Update" /V`. Install logs: `%LocalAppData%\SprkNotes\updates\install.log` (the SYSTEM profile for the task). Firewalls need the release host (`git.pwrs.dev`, or `github.com` and `api.github.com`).

### Early-release groups
Give the pilot group `UpdateChannel=beta` (or `alpha`) by policy or MSI switch; everyone else stays on `stable`. Moving a group back to stable keeps its current build until a newer stable release appears.

### Publishing a release (maintainers)
Follow [releasing.md](releasing.md): alpha first, then promote to beta or stable. It covers versions and tags, the CI draft, the Windows build and upload (`scripts/winvm.sh release-tag`, `scripts/publish-release.sh`), promotion, hotfixes and rollback. Releases need `update-manifest.json` (and `update-manifest.json.sig` when signed); Forgejo only accepts some file types, so installers carry a `.zip` suffix that the manifest records.
