# Intune deployment

Two options. Both install the same build.

## MSIX bundle (Line-of-business app)
Upload `SprkNotes_<ver>.msixbundle`. Per-user, no admin. The bundle holds x64 and arm64, so one assignment
covers the fleet. The signing certificate must chain to a root your devices trust.

## Win32 app (`.intunewin`)
`scripts/release.ps1` wraps the MSIs and `Install-SprkNotes.ps1` with IntuneWinAppUtil when it is on PATH.
- **Install command:** `powershell.exe -ExecutionPolicy Bypass -File Install-SprkNotes.ps1 -Machine`
- **Uninstall command:** `powershell.exe -ExecutionPolicy Bypass -File Install-SprkNotes.ps1 -Uninstall`
- **Detection rule:** file exists `%ProgramFiles%\SprkNotes\SprkNotes.exe` (machine) or registry
  `HKCU\Software\SprkNotes` value `Shortcut` (per user).

## Models
Deploy packs separately: `Install-SprkModel.ps1 -Pack <file.sprkmodel>` as SYSTEM. Policy keys are in
docs/admin-guide.md.
