// node --test tools/admin-form
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
const A = createRequire(import.meta.url)("./settings.js");

test("every setting has a unique key, a label and a known type", () => {
  const keys = A.SETTINGS.map((s) => s.key);
  assert.equal(new Set(keys).size, keys.length);
  for (const s of A.SETTINGS) {
    assert.ok(s.label && s.help, s.key);
    assert.ok(["path", "text", "url", "bool", "number", "enum", "list"].includes(s.type), s.key);
  }
  // Only settings with a control in the app take a starting value (InstallDefaults, UiManaged).
  const start = A.SETTINGS.filter((s) => s.start).map((s) => s.key).sort();
  assert.deepEqual(start, ["AssistantEnabled", "CaptionDelay", "CaptionSpeakers", "Hotkey", "HotkeyKeys", "LiveCaptions", "LiveLine", "MicPrompt", "Theme", "ThrowbackMode", "TranscriptsRoot", "UpdateChannel", "UpdateMode", "UpdateProvider", "UpdateRepoUrl", "WaveformStyle"]);
});

test("bad values are reported and good ones pass", () => {
  assert.deepEqual(A.validate([{ key: "TranscriptsRoot", mode: "lock", value: "%OneDrive%\\Notes" }]), []);
  assert.deepEqual(A.validate([{ key: "CommitDelaySeconds", mode: "lock", value: "5" }]), []);
  const bad = A.validate([
    { key: "TranscriptsRoot", mode: "lock", value: "Notes" },
    { key: "CommitDelaySeconds", mode: "lock", value: "40" },
    { key: "UpdateRepoUrl", mode: "start", value: "http://x" },
    { key: "CallContext", mode: "start", value: "1" },
    { key: "CopilotTarget", mode: "lock", value: 'a"b' },
    { key: "Nope", mode: "lock", value: "1" },
  ]);
  assert.equal(bad.length, 6);
});

test("packed values and the msiexec line keep variables as written", () => {
  const c = [
    { key: "TranscriptsRoot", mode: "start", value: "%OneDrive%\\Notes" },
    { key: "UpdatePublicKeys", mode: "lock", value: "ssh-ed25519 A\n\nssh-ed25519 B" },
  ];
  assert.equal(A.packed(c, "start"), "TranscriptsRoot=%OneDrive%\\Notes");
  assert.equal(A.packed(c, "lock"), "UpdatePublicKeys=ssh-ed25519 A|ssh-ed25519 B");
  assert.match(A.machineCommand(c), /ALLUSERS=1 .*POLICY="UpdatePublicKeys=ssh-ed25519 A\|ssh-ed25519 B" DEFAULTS="TranscriptsRoot=%OneDrive%\\Notes"/);
});

test("the SYSTEM script writes typed HKLM values and quotes safely", () => {
  const s = A.policyScript([
    { key: "TranscriptsRoot", mode: "lock", value: "%USERPROFILE%\\Bob's notes" },
    { key: "AssistantEnabled", mode: "lock", value: "0" },
    { key: "UpdateChannel", mode: "start", value: "beta" },
  ]);
  assert.match(s, /Kind = 'String'; Value = '%USERPROFILE%\\Bob''s notes'/);
  assert.match(s, /Name = 'AssistantEnabled'; Kind = 'DWord'; Value = \[int\]0/);
  assert.doesNotMatch(s, /UpdateChannel/);
  assert.match(s, /Registry64/);
  assert.match(s, /DeleteSubKeyTree/);
  assert.doesNotMatch(A.policyScript([], { replace: false }), /DeleteSubKeyTree/);
});

test("the SYSTEM script can write policy.json in a locked-down folder", () => {
  const c = [
    { key: "TranscriptsRoot", mode: "lock", value: "%OneDrive%\\Notes" },
    { key: "CommitDelaySeconds", mode: "lock", value: "7" },
    { key: "LiveCaptions", mode: "start", value: "1" },
    { key: "UpdatePublicKeys", mode: "lock", value: "ssh-ed25519 A\nssh-ed25519 B" },
  ];
  const json = JSON.parse(A.policyJson(c));
  assert.deepEqual(json.settings.TranscriptsRoot, { value: "%OneDrive%\\Notes", locked: true });
  assert.deepEqual(json.settings.CommitDelaySeconds, { value: 7, locked: true });
  assert.deepEqual(json.settings.LiveCaptions, { value: true, locked: false });
  assert.deepEqual(json.settings.UpdatePublicKeys.value, ["ssh-ed25519 A", "ssh-ed25519 B"]);

  const file = A.policyScript(c, { target: "file" });
  assert.doesNotMatch(file, /Registry64/);
  assert.match(file, /SetAccessRuleProtection\(\$true, \$false\)/);
  assert.match(file, /\/setowner '\*S-1-5-32-544'/);
  assert.match(file, /S-1-5-32-545'; Rights = 'ReadAndExecute'/);
  assert.match(file, /UTF8Encoding\(\$false\)/);
  // The JSON sits in a literal here-string, so nothing in it is expanded by PowerShell.
  assert.match(file, /\$json = @'\n\{[\s\S]*\n\}\n'@/);

  const both = A.policyScript(c, { target: "both" });
  assert.match(both, /Registry64/);
  assert.match(both, /policy\.json/);
  assert.match(both, /registry wins/);
  assert.doesNotMatch(A.policyScript(c), /policy\.json/);
});

test("the user script installs per user with the starting values baked in", () => {
  const s = A.installScript([{ key: "TranscriptsRoot", mode: "start", value: "%OneDrive%\\Notes" }], { channel: "alpha", tag: "v1" });
  assert.match(s, /\$Defaults = 'TranscriptsRoot=%OneDrive%\\Notes'/);
  assert.match(s, /\$Channel = 'alpha'/);
  assert.match(s, /\$Tag = 'v1'/);
  assert.match(s, /IsSystem/);
  assert.match(s, /SHA256SUMS/);
  assert.doesNotMatch(s, /ALLUSERS/);
});
