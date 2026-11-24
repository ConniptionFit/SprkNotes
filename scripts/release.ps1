<#
.SYNOPSIS
  Builds every SprkNotes artifact from one version: x64 and arm64 apps, MSIs, MSIX bundle,
  portable zip, install scripts, optional .intunewin, hashes and release-manifest.json.
.DESCRIPTION
  Run on Windows (the VM) from the repo root. The version comes only from the root Cargo.toml.
  Fails if any artifact reports a different version. Unsigned builds are dev-only.
.PARAMETER SherpaX64 / SherpaArm64
  Folders with the sherpa-onnx shared libraries per architecture (scripts/fetch-sherpa.sh --print
  win-x64-shared / win-arm64-shared on the dev host, copied over).
.PARAMETER ModelsDir
  A store folder of model packs to bundle read-only next to the app (transcription pack and the
  voice activity pack, each as <id>@<version>\ with model.json).
.PARAMETER CertThumbprint
  Code-signing certificate thumbprint in the Windows certificate store. Never put certificates
  or passwords in the repo.
.PARAMETER Publisher
  MSIX publisher, must equal the certificate subject (for example "CN=Contoso IT").
.PARAMETER UpdateSigningKey
  Path to the PUBLIC key file (.pub) of the key that signs update manifests. Signing goes through
  ssh-keygen and your ssh-agent (1Password), so no private key is read here. Without it the
  manifest is written unsigned, which only pilot builds accept.
.PARAMETER UploadSuffix
  Appended to the MSI file name in the update manifest. Forgejo only accepts some file types as
  release attachments, so MSIs are uploaded as `<name>.msi.zip` (default). Use '' for GitHub.
.PARAMETER TimestampUrl
  Optional RFC 3161 timestamp server used by signtool on the build host.
#>
[CmdletBinding()]
param(
  [string]$Out = 'dist',
  [string]$SherpaX64,
  [string]$SherpaArm64,
  [string]$ModelsDir,
  [string]$CertThumbprint = $env:SPRK_SIGN_THUMBPRINT,
  [string]$Publisher = 'CN=SprkNotes Dev',
  [string]$TimestampUrl,
  [string]$UpdateSigningKey,
  [string]$UploadSuffix = '.zip',
  [switch]$SkipMsix,
  [switch]$SkipMsi
)
$ErrorActionPreference = 'Stop'
Set-Location (Resolve-Path "$PSScriptRoot\..")

function Get-WorkspaceVersion {
  $in = $false
  foreach ($l in Get-Content Cargo.toml) {
    if ($l -match '^\[workspace\.package\]') { $in = $true; continue }
    if ($in -and $l -match '^\[') { break }
    if ($in -and $l -match '^version\s*=\s*"([^"]+)"') { return $Matches[1] }
  }
  throw 'No version in [workspace.package] of Cargo.toml'
}
$ver = Get-WorkspaceVersion
# MSI and MSIX versions are numbers only. A stable x.y.z stays x.y.z (MSIX: x.y.z.0); x.y.z-beta.N
# becomes x.y.z.(1000+N) and x.y.z-alpha.N becomes x.y.z.N (same mapping as
# sprk_update::Version::msi_version). Windows Installer ignores the fourth field when ordering
# upgrades, so the MSI allows same-version upgrades and the updater, not the MSI, decides order.
function ConvertTo-FourPart($v) {
  if ($v -match '^(\d+\.\d+\.\d+)$') { return $v }
  if ($v -match '^(\d+\.\d+\.\d+)-beta\.(\d+)$') { return "$($Matches[1]).$(1000 + [Math]::Min([int]$Matches[2], 999))" }
  if ($v -match '^(\d+\.\d+\.\d+)-alpha\.(\d+)$') { return "$($Matches[1]).$([Math]::Min([int]$Matches[2], 999))" }
  throw "Version '$v' must be x.y.z, x.y.z-beta.N or x.y.z-alpha.N"
}
$ver4 = ConvertTo-FourPart $ver
$msix4 = if ($ver4 -match '^\d+\.\d+\.\d+$') { "$ver4.0" } else { $ver4 }
$channel = if ($ver -like '*-alpha.*') { 'alpha' } elseif ($ver -like '*-beta.*') { 'beta' } else { 'stable' }
Write-Host "SprkNotes release $ver"

# --- Preflight: tools the build needs but an ssh session may not have on PATH ---
# The Windows OpenSSH service keeps the PATH it started with, so a tool installed later (winget) is
# invisible over ssh until the service restarts. Look in the usual places and add what we find.
function Find-Tool($name, $dirs) {
  if (Get-Command $name -ErrorAction SilentlyContinue) { return }
  foreach ($d in $dirs) {
    foreach ($hit in (Get-Item (Join-Path $d "$name.exe") -ErrorAction SilentlyContinue)) {
      $env:PATH = "$($hit.DirectoryName);$env:PATH"; Write-Host "using $name from $($hit.DirectoryName)"; return
    }
  }
  throw "$name was not found. Install it on the VM (for CMake: winget install Kitware.CMake), then run this again. It need not be on PATH: this script also looks in Program Files and in Visual Studio's own copy."
}
$vsRoots = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ } | ForEach-Object { Join-Path $_ 'Microsoft Visual Studio' }
$vsCmake = foreach ($r in $vsRoots) { Get-ChildItem "$r\*\*\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin" -Directory -ErrorAction SilentlyContinue }
Find-Tool 'cmake' (@("$env:ProgramFiles\CMake\bin", "${env:ProgramFiles(x86)}\CMake\bin", "$env:LOCALAPPDATA\Programs\CMake\bin") + @($vsCmake | ForEach-Object { $_.FullName }))
Find-Tool 'cargo' @("$env:USERPROFILE\.cargo\bin")
# arm64 helper: clang-cl (LLVM) and Ninja. winget puts Ninja under a Links or Packages folder.
$wg = "$env:LOCALAPPDATA\Microsoft\WinGet"
$ninjaDirs = @("$wg\Links", "$env:ProgramFiles\Ninja") + @(Get-ChildItem "$wg\Packages\Ninja-build.Ninja*" -Directory -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
Find-Tool 'ninja' $ninjaDirs
Find-Tool 'clang-cl' @("$env:ProgramFiles\LLVM\bin")
Find-Tool 'npm' @("$env:ProgramFiles\nodejs")
if (-not $SkipMsi) { Find-Tool 'wix' @("$env:USERPROFILE\.dotnet\tools") }

$arches = @(
  @{ Name = 'x64';   Triple = 'x86_64-pc-windows-msvc';  Sherpa = $SherpaX64 },
  @{ Name = 'arm64'; Triple = 'aarch64-pc-windows-msvc'; Sherpa = $SherpaArm64 }
)
$stage = Join-Path $Out 'stage'
Remove-Item -Recurse -Force $Out -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stage | Out-Null

# makeappx.exe lives in the Windows SDK and is usually not on PATH.
if (-not (Get-Command makeappx.exe -ErrorAction SilentlyContinue)) {
  $kit = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
  if ($kit) { $env:PATH = "$($kit.DirectoryName);$env:PATH" }
}

function Sign($file) {
  if (-not $CertThumbprint) { return }
  $a = @('sign', '/sha1', $CertThumbprint, '/fd', 'SHA256')
  if ($TimestampUrl) { $a += @('/tr', $TimestampUrl, '/td', 'SHA256') }
  & signtool.exe @a $file
  if ($LASTEXITCODE -ne 0) { throw "signtool failed for $file" }
}

# --- UI once, then the app and sprkctl per architecture ---
Push-Location app/ui; npm ci; npm run build; Pop-Location

foreach ($a in $arches) {
  if (-not $a.Sherpa) { throw "Missing -Sherpa$($a.Name) (sherpa-onnx shared libraries for $($a.Name))" }
  $env:SHERPA_ONNX_LIB_DIR = $a.Sherpa
  $dir = Join-Path $stage $a.Name
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  Push-Location app/src-tauri
  cargo build --release --target $a.Triple --features 'engine-shared,tauri/custom-protocol'
  if ($LASTEXITCODE -ne 0) { throw "app build failed ($($a.Name))" }
  Pop-Location
  cargo build --release -p sprkctl --target $a.Triple --features 'engine-shared,live'
  if ($LASTEXITCODE -ne 0) { throw "sprkctl build failed ($($a.Name))" }
  # The assistant helper (ADR-009): a separate low-priority program with no network code. It builds
  # on its own because it compiles llama.cpp. Always shipped; the assistant stays off until turned on.
  # llama.cpp refuses MSVC (cl) on ARM64 ("MSVC on ARM requires GGML_NATIVE=ON, use clang"), so the
  # arm64 helper is compiled with clang-cl from LLVM (winget install LLVM.LLVM) through Ninja
  # (winget install Ninja-build.Ninja). x64 keeps the default MSVC toolchain.
  $tk = $a.Triple.Replace('-', '_')
  if ($a.Name -eq 'arm64') {
    $llvm = Join-Path $env:ProgramFiles 'LLVM\bin'
    if (Test-Path $llvm) { $env:PATH = "$llvm;$env:PATH"; $env:LIBCLANG_PATH = $llvm }
    if (-not (Get-Command clang-cl -ErrorAction SilentlyContinue)) { throw 'arm64 assistant helper needs clang-cl: winget install LLVM.LLVM' }
    Set-Item "env:CC_$tk" 'clang-cl'; Set-Item "env:CXX_$tk" 'clang-cl'
    # clang-cl does not enable C++ exceptions by default; llama.cpp uses try/catch.
    Set-Item "env:CXXFLAGS_$tk" '/EHsc'
    # The Visual Studio generator ignores CC/CXX and picks cl.exe, which llama.cpp refuses on ARM64.
    # Ninja uses the clang-cl set above, so it is required here (winget install Ninja-build.Ninja).
    if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
      throw 'arm64 assistant helper needs Ninja so that CMake uses clang-cl instead of cl.exe: winget install Ninja-build.Ninja (then run again).'
    }
    $env:CMAKE_GENERATOR = 'Ninja'
    # A build dir configured earlier with another generator (e.g. Visual Studio) is reused and breaks
    # ("Skipping configuration step", then MSBuild cannot find install.vcxproj). Drop any that is not Ninja.
    Get-ChildItem "crates/sprk-assistant-llama/target/$($a.Triple)/release/build" -Directory -Filter 'llama-cpp-sys-2-*' -ErrorAction SilentlyContinue | ForEach-Object {
      $cache = Join-Path $_.FullName 'out/build/CMakeCache.txt'
      if ((Test-Path $cache) -and -not (Select-String -Path $cache -Pattern '^CMAKE_GENERATOR:INTERNAL=Ninja$' -Quiet)) {
        Write-Host "removing stale CMake build ($($_.Name)): configured with another generator"
        Remove-Item $_.FullName -Recurse -Force
      }
    }
  }
  cargo build --release --manifest-path crates/sprk-assistant-llama/Cargo.toml --bin sprk-assistant --target $a.Triple
  Remove-Item "env:CC_$tk", "env:CXX_$tk", "env:CXXFLAGS_$tk", env:CMAKE_GENERATOR -ErrorAction SilentlyContinue
  if ($LASTEXITCODE -ne 0) { throw "assistant helper build failed ($($a.Name))" }
  Copy-Item "crates/sprk-assistant-llama/target/$($a.Triple)/release/sprk-assistant.exe" $dir
  Copy-Item "app/src-tauri/target/$($a.Triple)/release/sprknotes.exe" (Join-Path $dir 'SprkNotes.exe')
  Copy-Item "target/$($a.Triple)/release/sprkctl.exe" $dir
  Get-ChildItem $a.Sherpa -Filter *.dll | Copy-Item -Destination $dir
  New-Item -ItemType Directory -Force -Path (Join-Path $dir 'prompts') | Out-Null
  Copy-Item prompts/summary.md (Join-Path $dir 'prompts')
  Copy-Item THIRD_PARTY_NOTICES.md, LICENSE $dir
  Copy-Item packaging/ninja/Install-SprkModel.ps1 $dir
  if ($ModelsDir) { Copy-Item $ModelsDir (Join-Path $dir 'models') -Recurse }
  Sign (Join-Path $dir 'SprkNotes.exe'); Sign (Join-Path $dir 'sprkctl.exe'); Sign (Join-Path $dir 'sprk-assistant.exe')
}

# Launcher for the portable zip (x64 build; runs emulated on ARM64 and starts the native build).
cargo build --release -p sprk-launcher --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'launcher build failed' }

$artifacts = @()

# --- MSI per architecture ---
if (-not $SkipMsi) {
  foreach ($a in $arches) {
    $msi = Join-Path $Out "SprkNotes_${ver}_$($a.Name).msi"
    # WiX v5 is pinned: v7+ refuses to build until a person accepts the OSMF EULA.
    wix build packaging/wix/Package.wxs -arch $a.Name -d "Version=$ver4" -d "StageDir=$((Resolve-Path (Join-Path $stage $a.Name)).Path)" -pdbtype none -o $msi
    if ($LASTEXITCODE -ne 0) { throw "wix build failed ($($a.Name))" }
    Sign $msi
    $artifacts += $msi
  }
}

# --- MSIX bundle (x64 + arm64) ---
if (-not $SkipMsix) {
  Add-Type -AssemblyName System.Drawing
  function Resize($src, $dst, $px) {
    $img = [System.Drawing.Image]::FromFile((Resolve-Path $src))
    $bmp = New-Object System.Drawing.Bitmap $px, $px
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = 'HighQualityBicubic'; $g.DrawImage($img, 0, 0, $px, $px)
    $bmp.Save($dst, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose(); $img.Dispose()
  }
  $bundleDir = Join-Path $Out 'msix'
  New-Item -ItemType Directory -Force -Path $bundleDir | Out-Null
  foreach ($a in $arches) {
    $pk = Join-Path $Out "msix-$($a.Name)"
    Copy-Item (Join-Path $stage $a.Name) $pk -Recurse
    $assets = Join-Path $pk 'Assets'; New-Item -ItemType Directory -Force -Path $assets | Out-Null
    Resize assets/icon-1024.png (Join-Path $assets 'StoreLogo.png') 50
    Resize assets/icon-1024.png (Join-Path $assets 'Square44x44Logo.png') 44
    Resize assets/icon-1024.png (Join-Path $assets 'Square150x150Logo.png') 150
    (Get-Content packaging/msix/AppxManifest.xml -Raw).
      Replace('{{VERSION}}', $msix4).Replace('{{ARCH}}', $a.Name).Replace('{{PUBLISHER}}', $Publisher) |
      Set-Content (Join-Path $pk 'AppxManifest.xml') -Encoding utf8
    $msix = Join-Path $bundleDir "SprkNotes_${ver}_$($a.Name).msix"
    makeappx.exe pack /d $pk /p $msix /o
    if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed ($($a.Name))" }
    Sign $msix
  }
  $bundle = Join-Path $Out "SprkNotes_$ver.msixbundle"
  makeappx.exe bundle /d $bundleDir /p $bundle /bv $msix4 /o
  if ($LASTEXITCODE -ne 0) { throw 'makeappx bundle failed' }
  Sign $bundle
  $artifacts += $bundle
}

# --- Portable zip ---
$pz = Join-Path $Out "portable"
New-Item -ItemType Directory -Force -Path $pz | Out-Null
Copy-Item (Join-Path $stage 'x64') (Join-Path $pz 'x64') -Recurse
Copy-Item (Join-Path $stage 'arm64') (Join-Path $pz 'arm64') -Recurse
Copy-Item target/x86_64-pc-windows-msvc/release/SprkNotes.exe $pz
Sign (Join-Path $pz 'SprkNotes.exe')
$zip = Join-Path $Out "SprkNotes_${ver}_portable.zip"
Compress-Archive -Path "$pz\*" -DestinationPath $zip
$artifacts += $zip

# --- Install scripts and optional Intune wrapper ---
Copy-Item packaging/ninja/Install-SprkNotes.ps1, packaging/ninja/Install-SprkModel.ps1 $Out
Copy-Item (Join-Path $stage 'x64/sprkctl.exe') (Join-Path $Out 'sprkctl.exe')
$artifacts += (Join-Path $Out 'Install-SprkNotes.ps1'), (Join-Path $Out 'Install-SprkModel.ps1')
if ((Get-Command IntuneWinAppUtil.exe -ErrorAction SilentlyContinue) -and -not $SkipMsi) {
  $iw = Join-Path $Out 'intune-src'
  New-Item -ItemType Directory -Force -Path $iw | Out-Null
  Copy-Item (Join-Path $Out '*.msi'), (Join-Path $Out 'Install-SprkNotes.ps1') $iw
  IntuneWinAppUtil.exe -c $iw -s Install-SprkNotes.ps1 -o $Out -q
  Rename-Item (Join-Path $Out 'Install-SprkNotes.intunewin') "SprkNotes_$ver.intunewin" -ErrorAction SilentlyContinue
}

# --- Alignment checks: every artifact reports the same version ---
$bad = @()
foreach ($a in $arches) {
  $exe = Join-Path $stage "$($a.Name)\SprkNotes.exe"
  $pv = (Get-Item $exe).VersionInfo.ProductVersion
  if ($pv -notlike "$ver*") { $bad += "$($a.Name) SprkNotes.exe is $pv" }
}
# The app is a Windows-subsystem exe: --version prints only when a console is attached, so an
# empty answer is not a failure here (its VersionInfo is checked above). A wrong answer is.
$x64 = & (Join-Path $stage 'x64\SprkNotes.exe') --version
if ($x64 -and $x64 -notmatch [regex]::Escape($ver)) { $bad += "x64 SprkNotes.exe --version said '$x64'" }
$ctl = & (Join-Path $stage 'x64\sprkctl.exe') --version
if ($ctl -notmatch [regex]::Escape($ver)) { $bad += "sprkctl --version said '$ctl'" }
if (-not $SkipMsi) {
  $inst = New-Object -ComObject WindowsInstaller.Installer
  foreach ($a in $arches) {
    $db = $inst.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $inst, @((Resolve-Path (Join-Path $Out "SprkNotes_${ver}_$($a.Name).msi")).Path, 0))
    $v = $db.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $db, @("SELECT Value FROM Property WHERE Property='ProductVersion'"))
    $v.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $v, $null) | Out-Null
    $r = $v.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $v, $null)
    $mv = $r.GetType().InvokeMember('StringData', 'GetProperty', $null, $r, 1)
    if ($mv -ne $ver4) { $bad += "$($a.Name) MSI ProductVersion is $mv" }
    # An MSI that harvested nothing still builds: require files in the File table.
    $fv = $db.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $db, @('SELECT File FROM File'))
    $fv.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $fv, $null) | Out-Null
    $fr = $fv.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $fv, $null)
    if (-not $fr) { $bad += "$($a.Name) MSI contains no files" }
  }
}
if (-not $SkipMsix) {
  foreach ($a in $arches) {
    $m = [xml](Get-Content (Join-Path $Out "msix-$($a.Name)/AppxManifest.xml") -Raw)
    if ($m.Package.Identity.Version -ne $msix4) { $bad += "$($a.Name) MSIX version is $($m.Package.Identity.Version)" }
  }
}
if ($bad.Count) { $bad | ForEach-Object { Write-Error $_ }; throw 'Version alignment failed. Nothing was released.' }

# --- Update manifest (what the in-app and scheduled-task updaters read; ADR-011) ---
if (-not $SkipMsi) {
  $pkgs = foreach ($a in $arches) {
    $f = Get-Item (Join-Path $Out "SprkNotes_${ver}_$($a.Name).msi")
    @{ arch = $a.Name; asset = "$($f.Name)$UploadSuffix"; sha256 = (Get-FileHash $f.FullName -Algorithm SHA256).Hash.ToLower(); size = $f.Length }
  }
  $um = Join-Path $Out 'update-manifest.json'
  @{ schema = 1; version = $ver; channel = $channel; notes_url = ''; packages = @($pkgs) } |
    ConvertTo-Json -Depth 4 | Set-Content $um -Encoding utf8NoBOM
  if ($UpdateSigningKey) {
    ssh-keygen -Y sign -f $UpdateSigningKey -n sprknotes-update $um
    if ($LASTEXITCODE -ne 0) { throw 'signing the update manifest failed' }
    Move-Item "$um.sig" (Join-Path $Out 'update-manifest.json.sig') -Force
  } else { Write-Warning 'Update manifest is unsigned (pass -UpdateSigningKey).' }
}
Copy-Item install.ps1 $Out

# --- Hashes and manifest ---
$files = Get-ChildItem $Out -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' -and $_.Name -ne 'release-manifest.json' }
$sums = $files | ForEach-Object { "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name }
$sums | Set-Content (Join-Path $Out 'SHA256SUMS.txt') -Encoding ascii
@{ version = $ver; signed = [bool]$CertThumbprint; artifacts = @($files | ForEach-Object { $_.Name }) } |
  ConvertTo-Json | Set-Content (Join-Path $Out 'release-manifest.json') -Encoding utf8
if (-not $CertThumbprint) { Write-Warning 'Unsigned build: development use only.' }
Write-Host "Release $ver complete in $Out"
