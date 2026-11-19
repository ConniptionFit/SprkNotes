<#
.SYNOPSIS
  Installs a transcription model pack for all users (machine scope). Run as SYSTEM or elevated.
.DESCRIPTION
  Wraps `sprkctl model install --scope machine`. Verifies the pack first (hashes, and the signature
  when required) and activates it at the next session start; nothing changes mid-session.
.PARAMETER Pack
  Path to the .sprkmodel file.
.PARAMETER Sprkctl
  Path to sprkctl.exe. Defaults to the one next to this script.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)][string]$Pack,
  [string]$Sprkctl = (Join-Path $PSScriptRoot 'sprkctl.exe')
)
$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Sprkctl)) { Write-Error "sprkctl.exe not found at $Sprkctl"; exit 2 }
if (-not (Test-Path $Pack)) { Write-Error "Pack not found: $Pack"; exit 2 }
& $Sprkctl model verify $Pack
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }          # 10 verify failed, 12 incompatible
& $Sprkctl model install $Pack --scope machine
exit $LASTEXITCODE                                        # 20 means not elevated
