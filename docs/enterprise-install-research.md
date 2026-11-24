# Enterprise install and updates: research and recommendations

Date: 2026-11-23. Decision record: [ADR-011](adr/011-app-updates.md). Built in the same change; items still to verify on real machines are listed at the end.

## The need
- A one-line install for a single user (no admin), with a Start menu entry.
- Updates from a repository (GitHub; Forgejo also supported), on a channel the user or admin picks (stable, beta, alpha).
- Admins can lock the channel and repository, force or disable updates, and preconfigure the app at install time or by policy.
- Per-machine installs (outside any user profile) update unobtrusively and need no logged-in user.

## Installer technology
| Option | Per-user | Per-machine | Install-time switches | Intune, GPO, SCCM | Rollback | Verdict |
|---|---|---|---|---|---|---|
| **MSI (WiX)** (already built) | Yes | Yes | Public properties (`UPDATE_CHANNEL=...`) and transforms | Native everywhere | Yes | **Recommended.** Nothing new to build or sign |
| MSIX | Yes | Provisioned for all users | No (config only through policy) | Intune LOB, needs a trusted certificate | Yes | Keep as an option. Cannot write HKLM, run SYSTEM tasks or take switches, so it cannot lock the repository at install |
| NSIS or Inno Setup exe | Yes | Yes | Any switch you write | Generic exe in Intune, detection by hand | No | A third installer to build, sign and keep in step. No advantage over MSI here |
| Portable zip | Yes | No | None | None | No | Stays for trials and USB use |

Windows Installer ignores the fourth version field when ordering upgrades. Beta and alpha builds differ only in that field, so the MSI allows same-version upgrades and the app decides what is newer.

## Updating
| Approach | Works per-user | Works per-machine without a user | Quiet | Lockable | Verdict |
|---|---|---|---|---|---|
| **App checks at launch, installs at exit** | Yes | No (needs the app open, and no elevation) | Yes | Yes (policy) | **Use for per-user** |
| **SYSTEM scheduled task running the app with `--update`** | n/a | Yes | Yes (no window, skips while the app is open) | Yes (same policy) | **Use for per-machine** (owner's choice) |
| Windows service | n/a | Yes | Yes | Yes | More code and attack surface than a task, no benefit at this scale |
| Intune, RMM or SCCM pushes each MSI | Yes | Yes | Yes | Central | Fully supported (`UPDATE_TASK=0` or `UpdateMode=off`). Admin does the work |
| MSIX `.appinstaller` | Yes | Per user | Yes | Weak | Needs a trusted certificate everywhere, no channel or repo lock |
| Tauri updater plugin | Yes | No | Yes | Weak | Brings its own TLS stack and key format and a JSON endpoint. We already have WinHTTP and SSHSIG, and the plugin cannot run unattended as SYSTEM |
| WinGet private source | Yes | Yes | Yes | Yes | Good future option for fleets. Needs a hosted REST source, so it is a follow-up |

## Trust
- **Manifest signature (chosen):** each release carries `update-manifest.json` plus an SSHSIG signature (namespace `sprknotes-update`), checked against keys in policy. It reuses the model-signing code, needs no purchased certificate, and stops a hostile mirror or a stolen host account from pushing a build, because the private key lives in the 1Password SSH agent and never on the host. Installer bytes must match the SHA-256 and size inside that signed manifest.
- **Authenticode:** worth adding when a code-signing certificate exists (removes SmartScreen prompts, lets WDAC and AppLocker publisher rules work). It does not replace the manifest signature.
- Hosts that accept only some file types (Forgejo here) get the MSI uploaded as `.msi.zip`; the manifest names the exact asset, so the app and `install.ps1` do not care.

## Controls for admins
Precedence, highest first: **policy** (HKLM, then HKCU) locks a setting and greys it out in Settings, then **the user's saved choice**, then **installer defaults** (written by the MSI switches; the user can change them), then built-in defaults (stable channel, notify mode, the SprkNotes repository).

| Goal | Do this |
|---|---|
| Lock channel and repository | `UpdateChannel` and `UpdateRepoUrl` in policy, or install with `LOCK_UPDATES=1` per machine |
| Force updates | `UpdateMode=force` (per-machine: installs when nobody has the app open) |
| Stop updates | `UpdateMode=off`: no update network calls at all |
| Early-release pilot group | `UpdateChannel=beta` or `alpha` for that group's policy |
| Internal mirror or GitHub | `UpdateRepoUrl` (+ `UpdateProvider=github` for GitHub Enterprise) |
| Preconfigure at install | `msiexec /i SprkNotes_<ver>_x64.msi ALLUSERS=1 /qn UPDATE_CHANNEL=beta UPDATE_MODE=auto LOCK_UPDATES=1` |
| Only trust your own builds | `UpdatePublicKeys` (SSH public key); unsigned manifests are then refused |

Every other existing policy key (model download, assistant features, transcripts root and the rest) keeps working the same way; the MSI switches here cover updates only. An ADMX template for Intune and Group Policy is the natural next step (`packaging/admx/` carries a first draft that has not been loaded into Intune or `gpedit` yet).

## Recommendations
1. Keep MSI as the one installer; do not add another technology.
2. Per-user: app-driven updates. Per-machine: SYSTEM scheduled task. Both read the same policy.
3. Publish `update-manifest.json` and its signature with every release (`scripts/release.ps1 -UpdateSigningKey <pub>`) and set the trusted key by policy at pilot sites.
4. Buy or arrange a code-signing certificate before 1.0 (SmartScreen, WDAC) and consider a WinGet source for fleets.

## Not yet verified on a real machine
- `install.ps1` end to end against a real release (no PowerShell in the build container).
- The MSI's registry switches and the scheduled task on the Windows VM (`msiexec /i ... ALLUSERS=1 UPDATE_CHANNEL=beta LOCK_UPDATES=1`, then `schtasks /Query /TN SprkNotes\Update`).
- A per-machine update run by the task, including the skip while the app is open.
- The ADMX draft in Intune and `gpedit`.
