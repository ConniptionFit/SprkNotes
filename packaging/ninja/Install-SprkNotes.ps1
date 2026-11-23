<#
.SYNOPSIS
  Installs (or removes) SprkNotes silently, picking the MSI that matches the OS architecture.
.DESCRIPTION
  Put this script next to SprkNotes_<ver>_x64.msi and SprkNotes_<ver>_arm64.msi. Use it from NinjaOne
  or any RMM. Logs to %ProgramData%\SprkNotes\install.log. Uninstall never deletes transcripts.
.PARAMETER Machine
  Install for all users (ALLUSERS=1, needs elevation). Default is per-user.
.PARAMETER Uninstall
  Remove SprkNotes.
.PARAMETER Channel / RepoUrl / Mode / PublicKey
  Preconfigure updates (MSI switches UPDATE_CHANNEL, UPDATE_REPO_URL, UPDATE_MODE, UPDATE_PUBLIC_KEY).
.PARAMETER Lock
  Write those values as admin policy so users cannot change them (LOCK_UPDATES=1; per-machine for a real lock).
.PARAMETER NoUpdateTask
  With -Machine, do not create the SYSTEM update task (UPDATE_TASK=0).
#>
[CmdletBinding()]
param(
  [switch]$Machine, [switch]$Uninstall,
  [ValidateSet('', 'stable', 'beta', 'alpha')][string]$Channel = '',
  [string]$RepoUrl = '',
  [ValidateSet('', 'off', 'notify', 'auto', 'force')][string]$Mode = '',
  [string]$PublicKey = '',
  [switch]$Lock, [switch]$NoUpdateTask)
$ErrorActionPreference = 'Stop'
$logDir = Join-Path $env:ProgramData 'SprkNotes'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$log = Join-Path $logDir 'install.log'
function Say($m) { "$(Get-Date -Format s) $m" | Tee-Object -FilePath $log -Append | Write-Host }

$arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
$tag = if ($arch -eq 'ARM64') { 'arm64' } else { 'x64' }
$msi = Get-ChildItem -Path $PSScriptRoot -Filter "SprkNotes_*_$tag.msi" | Sort-Object Name | Select-Object -Last 1
if (-not $msi) { Say "No $tag MSI found next to the script."; exit 2 }

$args = @('/i', "`"$($msi.FullName)`"", '/qn', '/norestart', '/l*v', "`"$logDir\msi-$tag.log`"")
if ($Uninstall) { $args[0] = '/x' }
elseif ($Machine) { $args += 'ALLUSERS=1' }
if (-not $Uninstall) {
  if ($Channel) { $args += "UPDATE_CHANNEL=$Channel" }
  if ($RepoUrl) { $args += "UPDATE_REPO_URL=`"$RepoUrl`"" }
  if ($Mode) { $args += "UPDATE_MODE=$Mode" }
  if ($PublicKey) { $args += "UPDATE_PUBLIC_KEY=`"$PublicKey`"" }
  if ($Lock) { $args += 'LOCK_UPDATES=1' }
  if ($NoUpdateTask) { $args += 'UPDATE_TASK=0' }
}
Say "msiexec $($args -join ' ')"
$p = Start-Process msiexec.exe -ArgumentList $args -Wait -PassThru
Say "exit code $($p.ExitCode)"
exit $p.ExitCode
