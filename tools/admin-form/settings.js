// SprkNotes admin form: the setting catalog and the script generators. Plain JavaScript with no
// imports and no network use, so index.html works opened straight from disk. The same file is
// loaded by `node --test` (settings.test.mjs) and checked against `sprk_config::KEYS` by a Rust
// test, so a new setting has to be added here too.
(function (root) {
  "use strict";

  // type: path | text | url | bool | number | enum | list
  // start: true when the installer may set a starting value (the user can change it later).
  const SETTINGS = [
    { key: "TranscriptsRoot", group: "Files", type: "path", start: true, label: "Save transcripts to", help: "Folder for new transcripts and the Library. Use %OneDrive%, %USERPROFILE% and the like so each person gets their own folder." },
    { key: "PromptTemplatePath", group: "Files", type: "path", label: "Summary prompt template", help: "A .md file used when sending a transcript to Claude or Copilot. Variables are expanded per user." },

    { key: "Theme", group: "Look and live text", type: "enum", options: ["system", "light", "dark"], start: true, label: "Appearance", help: "System follows Windows; Light or Dark always." },
    { key: "ThrowbackMode", group: "Look and live text", type: "bool", start: true, label: "Throwback Mode", help: "The first look of SprkNotes." },
    { key: "WaveformStyle", group: "Look and live text", type: "enum", options: ["bars", "rolling"], start: true, label: "Sound meter", help: "bars (nine bars, newest in the middle) or rolling (bars sliding right to left)." },
    { key: "LiveCaptions", group: "Look and live text", type: "bool", start: true, label: "Live captions", help: "Words on the pop-out while people are still speaking." },
    { key: "LiveLine", group: "Look and live text", type: "bool", start: true, label: "Live line in the transcript window", help: "The newest words grow at the end of the transcript before they settle." },
    { key: "CaptionDelay", group: "Look and live text", type: "enum", options: ["fast", "steady", "smooth"], start: true, label: "Live text delay", help: "fast, steady or smooth (smoothest)." },
    { key: "CaptionSpeakers", group: "Look and live text", type: "enum", options: ["others", "both"], start: true, label: "Whose words in the pop-out", help: "others only, or both (Others and Me)." },
    { key: "MicPrompt", group: "Look and live text", type: "bool", start: true, label: "Offer to start when another app uses the microphone", help: "Asks once per call while SprkNotes is in the background." },
    { key: "Hotkey", group: "Look and live text", type: "bool", start: true, label: "Keyboard shortcut", help: "Start, pause and resume from any app." },
    { key: "HotkeyKeys", group: "Look and live text", type: "text", start: true, label: "Shortcut keys", help: "For example Ctrl+Alt+S. Needs Ctrl, Alt or Win." },

    { key: "FillerRemoval", group: "Transcript", type: "bool", label: "Remove filler words", help: "Drop um, uh and similar from the transcript." },
    { key: "CommitDelaySeconds", group: "Transcript", type: "number", min: 3, max: 15, label: "Line delay (seconds)", help: "How long a line may still change before it is written, 3 to 15." },

    { key: "CalendarProvider", group: "Meetings", type: "enum", options: ["com", "off"], label: "Calendar source", help: "com reads the local Outlook calendar for meeting titles; off never looks." },
    { key: "CallContext", group: "Meetings", type: "bool", label: "Use call details", help: "Read the name of the call app and the meeting while a session runs." },
    { key: "CallPollSeconds", group: "Meetings", type: "number", min: 1, label: "Call check interval (seconds)", help: "How often call details are read." },
    { key: "NameOneToOneSpeakers", group: "Meetings", type: "bool", label: "Name the other person in 1:1 calls", help: "Use the other caller's name in place of Others." },
    { key: "CallMergeWindowSeconds", group: "Meetings", type: "number", min: 0, label: "Call merge window (seconds)", help: "Calls this close together count as one." },
    { key: "TeamsLocalApi", group: "Meetings", type: "bool", label: "Teams local API", help: "Ask the Teams client on this PC for meeting state." },
    { key: "AudioSourceDetection", group: "Meetings", type: "bool", label: "Detect audio apps", help: "Note which app is playing sound." },
    { key: "AudioMergeWindowSeconds", group: "Meetings", type: "number", min: 0, label: "Audio merge window (seconds)", help: "Audio app changes this close together count as one." },
    { key: "AudioSourceExcludePatterns", group: "Meetings", type: "list", label: "Ignore these audio apps", help: "One name pattern per line." },

    { key: "SummarizeEnabled", group: "Summaries", type: "bool", label: "Allow Summarize", help: "Off removes Summarize, so transcripts are never handed to Claude or Copilot. Transcripts may hold PHI." },
    { key: "ShowClaudeHandoff", group: "Summaries", type: "bool", label: "Show the Claude button", help: "Offer Claude for summaries." },
    { key: "ShowCopilotHandoff", group: "Summaries", type: "bool", label: "Show the Copilot button", help: "Offer Copilot for summaries." },
    { key: "CopilotTarget", group: "Summaries", type: "text", label: "Copilot target", help: "Where the Copilot button sends people (an https address or app link)." },

    { key: "AssistantEnabled", group: "Assistant", type: "bool", start: true, label: "On-device assistant", help: "Turn Ask Sprk and the other assistant features on or off." },
    { key: "AssistantTier", group: "Assistant", type: "enum", options: ["off", "titles", "light", "standard", "auto"], label: "Largest assistant size", help: "Caps the model size the assistant may use." },
    { key: "AssistantFeatures", group: "Assistant", type: "text", label: "Allowed features", help: "Comma separated, for example title,recap,actions,ask. Empty lets people choose." },
    { key: "AssistantPinnedModel", group: "Assistant", type: "text", label: "Pinned assistant model", help: "id@version to force." },
    { key: "AssistantIdleUnloadMinutes", group: "Assistant", type: "number", min: 0, label: "Unload after (minutes)", help: "Free the assistant's memory after this long idle." },
    { key: "AssistantMaxThreads", group: "Assistant", type: "number", min: 1, label: "Max threads", help: "Cap on CPU threads the assistant uses." },
    { key: "AssistantSaveOutputs", group: "Assistant", type: "bool", label: "Save assistant notes", help: "Let the assistant write notes next to the transcript." },
    { key: "AssistantBackground", group: "Assistant", type: "bool", label: "Work in the background", help: "Let the assistant run while a session is live." },

    { key: "ModelDownload", group: "Models", type: "bool", label: "Allow model downloads", help: "Off removes the Download model button and all model network use." },
    { key: "ModelUpdateCheck", group: "Models", type: "bool", label: "Check for newer models", help: "Look for a newer speech model at launch." },
    { key: "ModelIndexUrl", group: "Models", type: "url", label: "Model index address", help: "https address of an internal mirror's models-index.json." },
    { key: "RequireSignedModels", group: "Models", type: "bool", label: "Require signed models", help: "Refuse model packs without a trusted signature." },
    { key: "ModelPublicKeys", group: "Models", type: "list", label: "Model signing keys", help: "One allowed_signers line per line." },
    { key: "AllowedModelHashes", group: "Models", type: "list", label: "Allowed model hashes", help: "SHA-256 of model.json, one per line." },
    { key: "KeepModelVersions", group: "Models", type: "number", min: 1, label: "Model versions to keep", help: "Older versions are removed beyond this (default 2)." },

    { key: "UpdateMode", group: "Updates", type: "enum", options: ["off", "notify", "auto", "force"], start: true, label: "Update mode", help: "off, notify (ask), auto (install when not in use) or force." },
    { key: "UpdateChannel", group: "Updates", type: "enum", options: ["stable", "beta", "alpha"], start: true, label: "Release channel", help: "Which releases this group gets." },
    { key: "UpdateRepoUrl", group: "Updates", type: "url", start: true, label: "Release repository", help: "https address releases come from." },
    { key: "UpdateProvider", group: "Updates", type: "enum", options: ["github", "forgejo"], start: true, label: "Repository type", help: "Only when the address does not make it obvious." },
    { key: "UpdatePublicKeys", group: "Updates", type: "list", label: "Update signing keys", help: "ssh-ed25519 keys, one per line. Unsigned updates are refused when set." },
    { key: "UpdateCheckHours", group: "Updates", type: "number", min: 1, label: "Hours between update checks", help: "Default 24." },
  ];

  const POLICY_KEY = "SOFTWARE\\Policies\\SprkNotes";
  const PATH_VARS = ["%OneDrive%", "%OneDriveCommercial%", "%USERPROFILE%", "%LOCALAPPDATA%", "%APPDATA%", "%USERNAME%"];

  function byKey(key) {
    return SETTINGS.find((s) => s.key === key);
  }

  // A choice is { key, mode: "lock" | "start", value }. Lists hold their items one per line.
  function items(value) {
    return String(value)
      .split(/\r?\n/)
      .map((v) => v.trim())
      .filter((v) => v !== "");
  }

  function clean(choice) {
    const s = byKey(choice.key);
    const v = s && s.type === "list" ? items(choice.value) : String(choice.value ?? "").trim();
    return { ...choice, setting: s, value: v };
  }

  // Problems that would make a script fail or the app ignore a value; empty means fine.
  function validate(choices) {
    const errors = [];
    for (const raw of choices) {
      const c = clean(raw);
      const s = c.setting;
      if (!s) {
        errors.push(`${raw.key}: not a SprkNotes setting.`);
        continue;
      }
      if (c.mode === "start" && !s.start) errors.push(`${s.label}: can only be locked, not given a starting value.`);
      const values = Array.isArray(c.value) ? c.value : [c.value];
      if (values.length === 0 || values.every((v) => v === "")) {
        errors.push(`${s.label}: enter a value.`);
        continue;
      }
      for (const v of values) {
        if (/["\r\n]/.test(v)) errors.push(`${s.label}: double quotes are not allowed.`);
        if (v.includes(";") || v.includes("|")) errors.push(`${s.label}: ; and | are not allowed.`);
      }
      const v = values[0];
      if (s.type === "bool" && v !== "1" && v !== "0") errors.push(`${s.label}: choose on or off.`);
      if (s.type === "number") {
        const n = Number(v);
        if (!/^\d+$/.test(v) || (s.min !== undefined && n < s.min) || (s.max !== undefined && n > s.max)) {
          const range = s.max !== undefined ? `${s.min} to ${s.max}` : `${s.min ?? 0} or more`;
          errors.push(`${s.label}: enter a whole number, ${range}.`);
        }
      }
      if (s.type === "enum" && !s.options.includes(v)) errors.push(`${s.label}: pick one of ${s.options.join(", ")}.`);
      if (s.type === "url" && !/^https:\/\//i.test(v)) errors.push(`${s.label}: must start with https://.`);
      if (s.type === "path" && !(/^[A-Za-z]:\\/.test(v) || /^\\\\/.test(v) || /^%[A-Za-z0-9_()]+%/.test(v))) {
        errors.push(`${s.label}: use a full path such as D:\\Transcripts or %OneDrive%\\Transcripts.`);
      }
    }
    return errors;
  }

  // Key=Value;Key=Value, list items joined with |. What the MSI switches POLICY and DEFAULTS take.
  function packed(choices, mode) {
    return choices
      .map(clean)
      .filter((c) => c.mode === mode && c.setting)
      .map((c) => `${c.key}=${Array.isArray(c.value) ? c.value.join("|") : c.value}`)
      .join(";");
  }

  function ps(v) {
    return `'${String(v).replace(/'/g, "''")}'`;
  }

  function stamp(date) {
    return (date || new Date()).toISOString().slice(0, 10);
  }

  // The policy.json body: a value and a lock flag per setting (SPEC 10.2). Starting values go in
  // too, unlocked, so one file can carry both.
  function policyJson(choices) {
    const settings = {};
    for (const c of choices.map(clean)) {
      if (!c.setting || (c.mode !== "lock" && c.mode !== "start")) continue;
      const t = c.setting.type;
      const value = t === "bool" ? c.value === "1" : t === "number" ? Number(c.value) : c.value;
      settings[c.key] = { value, locked: c.mode === "lock" };
    }
    return JSON.stringify({ settings }, null, 2);
  }

  // Locked settings, from a script that runs as SYSTEM. Users cannot write HKLM or the locked-down
  // ProgramData file, so this is the lock even when the app itself is installed per user. Paths
  // keep their %VARIABLES%; the app expands them for whoever runs it.
  // opts.target: "registry" (HKLM values), "file" (%ProgramData%\SprkNotes\policy.json) or "both".
  // When both hold a setting, the registry wins.
  function policyScript(choices, opts = {}) {
    const locked = choices.map(clean).filter((c) => c.mode === "lock" && c.setting);
    const replace = opts.replace !== false;
    const target = ["registry", "file", "both"].includes(opts.target) ? opts.target : "registry";
    const toRegistry = target !== "file";
    const toFile = target !== "registry";
    const lines = locked.map((c) => {
      const s = c.setting;
      if (s.type === "list") return `  @{ Name = ${ps(c.key)}; Kind = 'MultiString'; Value = [string[]]@(${c.value.map(ps).join(", ")}) }`;
      if (s.type === "bool" || s.type === "number") return `  @{ Name = ${ps(c.key)}; Kind = 'DWord'; Value = [int]${Number(c.value)} }`;
      return `  @{ Name = ${ps(c.key)}; Kind = 'String'; Value = ${ps(c.value)} }`;
    });
    const where = [toRegistry ? "HKLM\\" + POLICY_KEY : "", toFile ? "%ProgramData%\\SprkNotes\\policy.json" : ""].filter(Boolean).join(" and ");
    const registry = !toRegistry ? [] : [
      "",
      "# Registry: HKLM, 64-bit view so a 32-bit PowerShell writes the key the app reads.",
      "$values = @(",
      lines.join(",\n") || "  # (nothing locked)",
      ")",
      "$hklm = [Microsoft.Win32.RegistryKey]::OpenBaseKey('LocalMachine', 'Registry64')",
      replace ? `$hklm.DeleteSubKeyTree(${ps(POLICY_KEY)}, $false)` : "",
      `$key = $hklm.CreateSubKey(${ps(POLICY_KEY)})`,
      "foreach ($v in $values) { $key.SetValue($v.Name, $v.Value, [Microsoft.Win32.RegistryValueKind]$v.Kind) }",
      "$key.Close()",
      "Write-Output \"SprkNotes policy: $($values.Count) setting(s) locked in the registry.\"",
    ];
    const file = !toFile ? [] : [
      "",
      "# File: %ProgramData%\\SprkNotes\\policy.json, always written whole. The app uses it only when",
      "# the folder and the file are owned by Administrators or SYSTEM and nobody else can change",
      "# them, so the folder is locked down first (SYSTEM and Administrators full control, Users read).",
      "$json = @'",
      policyJson(choices),
      "'@",
      "$dir = Join-Path $env:ProgramData 'SprkNotes'",
      "$file = Join-Path $dir 'policy.json'",
      "New-Item -ItemType Directory -Force -Path $dir | Out-Null",
      "function Lock-Path([string]$Path, [bool]$Folder) {",
      "  & icacls.exe $Path /setowner '*S-1-5-32-544' /C /Q | Out-Null",
      "  if ($LASTEXITCODE -ne 0) { throw \"Could not make Administrators the owner of $Path.\" }",
      "  $sec = if ($Folder) { New-Object Security.AccessControl.DirectorySecurity } else { New-Object Security.AccessControl.FileSecurity }",
      "  $sec.SetAccessRuleProtection($true, $false)   # no inherited entries, so Users can't add files",
      "  $inherit = if ($Folder) { [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit' } else { [Security.AccessControl.InheritanceFlags]::None }",
      "  $rules = @(",
      "    @{ Sid = 'S-1-5-18'; Rights = 'FullControl' },",
      "    @{ Sid = 'S-1-5-32-544'; Rights = 'FullControl' },",
      "    @{ Sid = 'S-1-5-32-545'; Rights = 'ReadAndExecute' }",
      "  )",
      "  foreach ($r in $rules) {",
      "    $sid = New-Object Security.Principal.SecurityIdentifier($r.Sid)",
      "    $sec.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid, $r.Rights, $inherit, 'None', 'Allow')))",
      "  }",
      "  # Writes only the access list (the owner was set above), replacing every earlier entry.",
      "  (Get-Item -LiteralPath $Path -Force).SetAccessControl($sec)",
      "}",
      "Lock-Path $dir $true",
      "if (Test-Path -LiteralPath $file) { Remove-Item -LiteralPath $file -Force }",
      "[IO.File]::WriteAllText($file, $json, (New-Object Text.UTF8Encoding($false)))",
      "Lock-Path $file $false",
      "Write-Output 'SprkNotes policy: wrote policy.json.'",
    ];
    return [
      "# SprkNotes locked settings. Generated by the SprkNotes admin form on " + stamp(opts.date) + ".",
      "# Run as SYSTEM (NinjaOne: Run As 'System'). Writes " + where + ",",
      "# which only administrators can change; SprkNotes shows these settings as managed by your",
      "# administrator." + (toRegistry && toFile ? " Where both hold a setting, the registry wins." : ""),
      toRegistry
        ? replace
          ? "# Replaces every SprkNotes registry policy value on the PC, so running it again is safe."
          : "# Adds or updates these registry values and leaves other SprkNotes policy values as they are."
        : "",
      "# To remove SprkNotes policy later, run as SYSTEM:" +
        (toRegistry ? " Remove-Item 'HKLM:\\" + POLICY_KEY + "' -Recurse;" : "") +
        (toFile ? " Remove-Item \"$env:ProgramData\\SprkNotes\\policy.json\"" : ""),
      "$ErrorActionPreference = 'Stop'",
      "$me = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()",
      "if (-not $me.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {",
      "  Write-Output 'Run this as SYSTEM or an administrator.'; exit 5",
      "}",
      ...registry,
      ...file,
      "exit 0",
      "",
    ]
      .filter((l) => l !== "")
      .join("\n")
      .replace(/\n?$/, "\n");
  }

  // Installs SprkNotes for the person who is signed in (no admin rights needed, into their own
  // profile), with the starting values baked in. Locked values come from policyScript instead.
  function installScript(choices, opts = {}) {
    const defaults = packed(choices, "start");
    const repo = (opts.repo || "ConniptionFit/SprkNotes").trim();
    const channel = opts.channel || "stable";
    const tag = (opts.tag || "").trim();
    const folder = (opts.folder || "").trim();
    return [
      "# SprkNotes install for the signed-in user. Generated by the SprkNotes admin form on " + stamp(opts.date) + ".",
      "# Run as the logged-on user (NinjaOne: Run As 'Current Logged on User'), for example from the",
      "# self-service portal. Installs into the user's own profile; no admin rights needed. Safe to run",
      "# again: it upgrades in place and keeps the user's transcripts and choices.",
      "$ErrorActionPreference = 'Stop'",
      "$ProgressPreference = 'SilentlyContinue'",
      `$Defaults = ${ps(defaults)}   # starting values; the user may change them in Settings`,
      `$Repo = ${ps(repo)}`,
      `$Channel = ${ps(channel)}   # newest release on this channel when no tag is given`,
      `$Tag = ${ps(tag)}`,
      `$Folder = ${ps(folder)}   # a folder or share holding the MSIs; empty downloads from GitHub`,
      "",
      "if ([Security.Principal.WindowsIdentity]::GetCurrent().IsSystem) {",
      "  Write-Output 'Run this as the logged-on user, not SYSTEM: SprkNotes installs into the user profile.'; exit 6",
      "}",
      "$work = Join-Path $env:LOCALAPPDATA 'SprkNotes\\install'",
      "New-Item -ItemType Directory -Force -Path $work | Out-Null",
      "$log = Join-Path $work 'install.log'",
      "function Say($m) { \"$(Get-Date -Format s) $m\" | Tee-Object -FilePath $log -Append | Write-Output }",
      "$arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }",
      "$cpu = if ($arch -eq 'ARM64') { 'arm64' } else { 'x64' }",
      "",
      "if ($Folder) {",
      "  $msi = Get-ChildItem -Path $Folder -Filter \"SprkNotes_*_$cpu.msi\" | Sort-Object Name | Select-Object -Last 1",
      "  if (-not $msi) { Say \"No $cpu MSI in $Folder.\"; exit 2 }",
      "  $msiPath = $msi.FullName",
      "} else {",
      "  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12",
      "  $api = \"https://api.github.com/repos/$Repo/releases\"",
      "  $headers = @{ 'User-Agent' = 'SprkNotes-installer'; 'Accept' = 'application/vnd.github+json' }",
      "  if ($Tag) { $release = Invoke-RestMethod -Headers $headers -Uri \"$api/tags/$Tag\" }",
      "  else {",
      "    $ok = { param($t) switch ($Channel) { 'alpha' { $true } 'beta' { $t -notmatch '-alpha' } default { $t -notmatch '-' } } }",
      "    # Assigned first: Windows PowerShell 5.1 passes a JSON array down a pipe as one item.",
      "    $all = Invoke-RestMethod -Headers $headers -Uri \"${api}?per_page=30\"",
      "    $release = $all | Where-Object { -not $_.draft -and (& $ok $_.tag_name) } | Select-Object -First 1",
      "  }",
      "  if (-not $release) { Say \"No $Channel release found in $Repo.\"; exit 3 }",
      "  $asset = $release.assets | Where-Object { $_.name -like \"SprkNotes_*_$cpu.msi\" } | Select-Object -First 1",
      "  $sums = $release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' } | Select-Object -First 1",
      "  if (-not $asset -or -not $sums) { Say \"$($release.tag_name) has no $cpu MSI or checksum list.\"; exit 3 }",
      "  $msiPath = Join-Path $work $asset.name",
      "  Say \"Downloading $($asset.name) from $($release.tag_name)\"",
      "  Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri $asset.browser_download_url -OutFile $msiPath",
      "  $sumsPath = Join-Path $work 'SHA256SUMS.txt'",
      "  Invoke-WebRequest -UseBasicParsing -Headers $headers -Uri $sums.browser_download_url -OutFile $sumsPath",
      "  $line = Get-Content -Path $sumsPath | Where-Object { $_ -match \"\\s$([regex]::Escape($asset.name))\\s*$\" } | Select-Object -First 1",
      "  $want = if ($line) { ($line -split '\\s+')[0] } else { '' }",
      "  $got = (Get-FileHash -Algorithm SHA256 -Path $msiPath).Hash",
      "  if (-not $want -or $got -ne $want) { Say \"Checksum mismatch for $($asset.name); not installing.\"; Remove-Item $msiPath; exit 4 }",
      "}",
      "",
      "$msiArgs = @('/i', \"`\"$msiPath`\"\", '/qn', '/norestart', '/l*v', \"`\"$work\\msi.log`\"\")",
      "if ($Defaults) { $msiArgs += \"DEFAULTS=`\"$Defaults`\"\" }",
      "Say \"Installing $(Split-Path $msiPath -Leaf) for $env:USERNAME\"",
      "$p = Start-Process msiexec.exe -ArgumentList $msiArgs -Wait -PassThru",
      "Say \"msiexec exit code $($p.ExitCode)\"",
      "if (-not $Folder) { Remove-Item $msiPath -ErrorAction SilentlyContinue }",
      "exit $p.ExitCode",
      "",
    ].join("\n");
  }

  // One msiexec line for tools that push MSIs per machine (Intune, SCCM, GPO). A per-machine
  // install writes POLICY to HKLM, so it locks; DEFAULTS stay changeable.
  function machineCommand(choices) {
    const pol = packed(choices, "lock");
    const def = packed(choices, "start");
    let line = "msiexec /i SprkNotes_<version>_x64.msi ALLUSERS=1 /qn /norestart";
    if (pol) line += ` POLICY="${pol}"`;
    if (def) line += ` DEFAULTS="${def}"`;
    return line;
  }

  const api = { SETTINGS, PATH_VARS, POLICY_KEY, byKey, validate, packed, policyJson, policyScript, installScript, machineCommand };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else root.SprkAdmin = api;
})(typeof window !== "undefined" ? window : globalThis);
