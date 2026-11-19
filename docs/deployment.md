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
Policy is read when a session starts. Values it cannot parse are ignored and the default applies. Registry policy for the transcripts root, commit delay, filler removal, audio sources and call context is wired in; other keys are read as the matching features land.

## Verify a deployment
```powershell
(Get-Item "$env:ProgramFiles\SprkNotes\SprkNotes.exe").VersionInfo.ProductVersion
& "$env:ProgramFiles\SprkNotes\sprkctl.exe" --version
& "$env:ProgramFiles\SprkNotes\sprkctl.exe" model list --json
```
Compare the version with `release-manifest.json` and check file hashes against `SHA256SUMS.txt`.

## Remove
MSI or script uninstall as above; MSIX via `Remove-AppxPackage`; portable by deleting the folder. Transcripts are never deleted by uninstalling. Policy keys stay until you remove them.
