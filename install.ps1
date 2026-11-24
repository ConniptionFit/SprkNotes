<#
.SYNOPSIS
  One-line SprkNotes installer. Installs for the current user (no admin), adds a Start menu entry.
.DESCRIPTION
  Quick install:
    irm https://raw.githubusercontent.com/ConniptionFit/SprkNotes/main/install.ps1 | iex

  With options (a piped script cannot take parameters, so wrap it):
    & ([scriptblock]::Create((irm https://raw.githubusercontent.com/ConniptionFit/SprkNotes/main/install.ps1))) -Channel beta

  The script finds the newest release on the channel, downloads the MSI for this PC (x64 or ARM64),
  checks its SHA-256 against the release's update-manifest.json, and runs msiexec. Existing
  transcripts and settings are never touched. Nothing is sent to the release host except plain GETs.

  Environment variables work too (handy for RMM tools): SPRK_CHANNEL, SPRK_REPO_URL, SPRK_MODE.
.PARAMETER Channel
  stable (default), beta or alpha. Beta includes stable releases, alpha includes both.
.PARAMETER RepoUrl
  Where releases come from. Default: the SprkNotes repository on GitHub (keep this default in step with distribution.toml). GitHub addresses work too.
.PARAMETER Mode
  off, notify (default for one user), auto or force: how updates are applied after install.
.PARAMETER Machine
  Install for all users (needs an elevated PowerShell). Also creates the SYSTEM update task,
  so the install keeps itself current without prompts, and starts in auto mode.
.PARAMETER Lock
  Write Channel, RepoUrl and Mode as admin policy so users cannot change them in Settings.
  Needs -Machine to be a real lock (per-user policy sits in the user's own registry hive).
.PARAMETER PublicKey
  An ssh-ed25519 public key allowed to sign update manifests. Makes the app reject unsigned updates.
.PARAMETER NoUpdateTask
  With -Machine: do not create the scheduled update task.
.PARAMETER Uninstall
  Remove SprkNotes (transcripts are kept).
#>
[CmdletBinding()]
param(
  [ValidateSet('stable', 'beta', 'alpha')][string]$Channel = $(if ($env:SPRK_CHANNEL) { $env:SPRK_CHANNEL } else { 'stable' }),
  [string]$RepoUrl = $(if ($env:SPRK_REPO_URL) { $env:SPRK_REPO_URL } else { 'https://github.com/ConniptionFit/SprkNotes' }),
  [ValidateSet('', 'off', 'notify', 'auto', 'force')][string]$Mode = $(if ($env:SPRK_MODE) { $env:SPRK_MODE } else { '' }),
  [switch]$Machine,
  [switch]$Lock,
  [string]$PublicKey,
  [switch]$NoUpdateTask,
  [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13

function Say($m) { Write-Host "SprkNotes: $m" }
$arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
$tag = if ($arch -eq 'ARM64') { 'arm64' } else { 'x64' }

if ($Machine) {
  $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  if (-not $admin) { throw 'Run an elevated PowerShell for -Machine.' }
}

if ($Uninstall) {
  $key = Get-ChildItem 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall', 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue |
    Get-ItemProperty | Where-Object { $_.DisplayName -eq 'SprkNotes' } | Select-Object -First 1
  if (-not $key) { Say 'Not installed.'; return }
  $p = Start-Process msiexec.exe -ArgumentList '/x', $key.PSChildName, '/qn', '/norestart' -Wait -PassThru
  Say "Removed (exit code $($p.ExitCode)). Transcripts were kept."
  return
}

# --- repository address -> releases API ---
if ($RepoUrl -notmatch '^https://([A-Za-z0-9.\-:]+)/([A-Za-z0-9._\-]+)/([A-Za-z0-9._\-]+?)(\.git)?/?$') { throw "RepoUrl must look like https://host/owner/repo (got '$RepoUrl')." }
$hostName, $owner, $repo = $Matches[1], $Matches[2], $Matches[3]
$api = if ($hostName -eq 'github.com') { "https://api.github.com/repos/$owner/$repo/releases?per_page=50" }
       else { "https://$hostName/api/v1/repos/$owner/$repo/releases?limit=50" }

# --- newest release on the channel (same rules as the app: alpha < beta < stable) ---
function Get-Rank($t) {
  if ($t -notmatch '^v?(\d+)\.(\d+)\.(\d+)(?:-(alpha|beta)\.(\d+))?$') { return $null }
  $stage = switch ($Matches[4]) { 'alpha' { 0 } 'beta' { 1 } default { 2 } }
  $n = if ($Matches[5]) { [int]$Matches[5] } else { 0 }
  [pscustomobject]@{ Stage = $stage; Key = ('{0:D6}.{1:D6}.{2:D6}.{3}.{4:D6}' -f [int]$Matches[1], [int]$Matches[2], [int]$Matches[3], $stage, $n) }
}
$minStage = @{ stable = 2; beta = 1; alpha = 0 }[$Channel]   # lowest release stage this channel accepts
Say "Looking for the newest $Channel release..."
$releases = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'SprkNotes-install' }
$pick = $releases | Where-Object { -not $_.draft -and ($_.assets | Where-Object name -eq 'update-manifest.json') } |
  ForEach-Object {
    $r = Get-Rank $_.tag_name
    if ($r -and $r.Stage -ge $minStage) { [pscustomobject]@{ Rel = $_; Key = $r.Key } }
  } | Sort-Object Key -Descending | Select-Object -First 1
if (-not $pick) { throw "No $Channel release with an update manifest was found at $RepoUrl." }
$rel = $pick.Rel
Say "Found $($rel.tag_name)."

$manAsset = $rel.assets | Where-Object name -eq 'update-manifest.json' | Select-Object -First 1
$manBytes = (Invoke-WebRequest -Uri $manAsset.browser_download_url -UseBasicParsing -Headers @{ 'User-Agent' = 'SprkNotes-install' }).Content
$manText = if ($manBytes -is [byte[]]) { [Text.Encoding]::UTF8.GetString($manBytes) } else { [string]$manBytes }
$man = $manText | ConvertFrom-Json
$pkg = $man.packages | Where-Object arch -eq $tag | Select-Object -First 1
if (-not $pkg) { throw "This release has no $tag installer." }
$asset = $rel.assets | Where-Object name -eq $pkg.asset | Select-Object -First 1
if (-not $asset) { throw "Release asset '$($pkg.asset)' is missing." }

$dir = Join-Path ([IO.Path]::GetTempPath()) ("sprknotes-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $dir | Out-Null
try {
  $msi = Join-Path $dir "SprkNotes_$($man.version)_$tag.msi"
  Say "Downloading $($asset.name) ($([math]::Round($pkg.size / 1MB, 1)) MB)..."
  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $msi -UseBasicParsing -Headers @{ 'User-Agent' = 'SprkNotes-install' }
  $hash = (Get-FileHash $msi -Algorithm SHA256).Hash.ToLower()
  if ($hash -ne $pkg.sha256.ToLower()) { throw 'The download did not match its checksum. Nothing was installed.' }

  $props = @()
  if ($Machine) { $props += 'ALLUSERS=1' }
  if ($Machine -and $NoUpdateTask) { $props += 'UPDATE_TASK=0' }
  # Written only when asked for, so a plain install leaves the app on its own defaults.
  if ($PSBoundParameters.ContainsKey('Channel') -or $env:SPRK_CHANNEL -or $Lock) { $props += "UPDATE_CHANNEL=$Channel" }
  if ($PSBoundParameters.ContainsKey('RepoUrl') -or $env:SPRK_REPO_URL -or $Lock) { $props += "UPDATE_REPO_URL=`"$RepoUrl`"" }
  if ($Mode) { $props += "UPDATE_MODE=$Mode" }
  if ($PublicKey) { $props += "UPDATE_PUBLIC_KEY=`"$PublicKey`"" }
  if ($Lock) { $props += 'LOCK_UPDATES=1' }

  $log = Join-Path $dir 'install.log'
  $args = @('/i', "`"$msi`"", '/qn', '/norestart', '/l*v', "`"$log`"") + $props
  Say "Installing $($man.version) for $(if ($Machine) { 'all users' } else { 'you' })..."
  $p = Start-Process msiexec.exe -ArgumentList $args -Wait -PassThru
  if ($p.ExitCode -ne 0 -and $p.ExitCode -ne 3010) {
    $keep = Join-Path ([IO.Path]::GetTempPath()) 'sprknotes-install.log'
    Copy-Item $log $keep -ErrorAction SilentlyContinue
    throw "msiexec failed with exit code $($p.ExitCode). Log: $keep"
  }
  Say "Done. Open SprkNotes from the Start menu. Open Settings and press Download model if no model is installed."
} finally {
  Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
}
