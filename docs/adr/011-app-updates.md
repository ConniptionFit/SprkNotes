# ADR-011: App updates, release channels and enterprise install

Status: **accepted** (owner request, 2026-11-23). Amends hard rule 2 ("no app update checks"). Research and the options compared: [enterprise-install-research.md](../enterprise-install-research.md).

## Context
SprkNotes had no way to update itself. The owner wants a one-line per-user install, automatic updates from the
repository's releases, release channels (stable, beta, alpha) so some users can run early builds, and, for
enterprise use, a way for admins to lock the channel and repository, force or disable updates, preconfigure
the app at install time, and keep per-machine installs current without prompts.

## Decision
1. **Installer stays MSI (WiX).** One MSI per architecture, per-user or per-machine, with public install
   switches. MSIX and the portable zip are unchanged. No new installer technology.
2. **Release source abstraction.** `sprk-update` knows two providers, Forgejo (default, `git.pwrs.dev`) and
   GitHub (github.com or GitHub Enterprise). Both return the same release fields, so one parser serves both; only the
   list-releases address differs. Adding another host means adding an address builder.
3. **Channels from the tag.** `vX.Y.Z` is stable, `vX.Y.Z-beta.N` is beta, `vX.Y.Z-alpha.N` is alpha. Stable
   users see stable only, beta users see stable and beta, alpha users see everything. Tags that match nothing
   (`v0.0.1-pilot1`, `models`) and drafts are ignored. The updater never offers a downgrade.
4. **Signed update manifest.** Each release carries `update-manifest.json` (version, channel, and per architecture the
   installer's asset name, SHA-256 and size) and `update-manifest.json.sig`, an OpenSSH SSHSIG with the namespace
   `sprknotes-update` (a model signature cannot pass as an update signature). The key stays in the 1Password SSH agent. Trusted
   keys come from policy `UpdatePublicKeys`; once any key is set, or from 1.0, unsigned manifests are refused.
   Installers must match the SHA-256 and size in the signed manifest before msiexec runs.
5. **Policy and install switches.** New keys: `UpdateMode` (`off`, `notify`, `auto`, `force`), `UpdateChannel`,
   `UpdateRepoUrl`, `UpdateProvider`, `UpdatePublicKeys`, `UpdateCheckHours`. A key set by policy locks the matching Settings control.
   The MSI writes them from `UPDATE_*` switches: as **Defaults** (`HKMU\Software\SprkNotes\Defaults`, user may change) or, with
   `LOCK_UPDATES=1`, as **policy** (`HKMU\Software\Policies\SprkNotes`, locked). Per-machine installs write HKLM, which standard users cannot change.
   Precedence: policy, then the user's choice, then installer defaults, then built-in defaults (stable, notify, git.pwrs.dev).
6. **Per-user installs update from the app.** At launch (spaced by `UpdateCheckHours`, default 24) the app checks. `notify`
   shows the version and lets the user choose "Update when I close the app". `auto` and `force` download and verify in the
   background and install silently when the app closes. Never mid-session.
7. **Per-machine installs update from a SYSTEM scheduled task.** The MSI registers `SprkNotes\Update` (every 6 hours, SYSTEM, no window)
   that runs `SprkNotes.exe --update`. It checks, downloads, verifies, and installs only when the mode is `auto` or `force` and no
   SprkNotes window is open; otherwise the verified installer waits for the next run. Per-machine installs default to `auto`. `UPDATE_TASK=0`
   leaves the task out for sites that push updates with Intune or an RMM.
8. **One-line install.** `install.ps1` at the repository root finds the newest release on the channel, checks the SHA-256 and runs the MSI
   per user. Parameters cover channel, repository, mode, per-machine, locking, key and uninstall.

## Guards
- https only; WinHTTP on Windows (system proxy and certificate store), same client as model downloads. Plain GETs, nothing about the user or PC sent.
- `UpdateMode=off` makes no network call for updates at all.
- Nothing is installed during a session. A failed or rejected update changes nothing (Windows Installer rolls back).
- Update packages are never run unless the SHA-256 matches the manifest; with keys configured the manifest must also verify.
- Logs hold versions and counts only.

## Consequences
- CLAUDE.md rule 2, SPEC 7 (network row), the admin guide privacy text and the SPEC checklist change: outbound traffic is allowed for the model index and packs (ADR-005) **and** the update check and installer (this ADR). Telemetry, analytics and crash upload stay forbidden.
- Firewalls need the release host (`git.pwrs.dev`, or `github.com` and `api.github.com`) allowed, or `UpdateMode=off`.
- Version rule: the workspace version may carry `-beta.N` or `-alpha.N`. `release.ps1` maps it to MSI and MSIX numbers.
- Windows Installer orders upgrades by the first three version fields only, so the MSI allows same-version upgrades and the updater, not the MSI, decides what is newer.
- Open: a code-signing certificate for Authenticode (SmartScreen), an ADMX template tested in Intune, and a private WinGet source are separate follow-ups.
