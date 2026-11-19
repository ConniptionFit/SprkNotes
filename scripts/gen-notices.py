#!/usr/bin/env python3
"""Regenerate the license summary in THIRD_PARTY_NOTICES.md from `cargo metadata`.
Fails if any dependency has no license field (release gate)."""
import json, subprocess, collections, re, sys, pathlib

root = pathlib.Path(__file__).resolve().parent.parent
meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--locked"], cwd=root))
ws = set(meta["workspace_members"])
counts, missing, rows = collections.Counter(), [], []
for p in meta["packages"]:
    if p["id"] in ws:
        continue
    lic = p.get("license")
    if not lic:
        missing.append(p["name"])
        continue
    counts[lic] += 1
    rows.append((p["name"], p["version"], lic))
if missing:
    sys.exit("missing license: " + ", ".join(sorted(missing)))
path = root / "THIRD_PARTY_NOTICES.md"
text = path.read_text()
head = text.split("## License summary (Rust crates)")[0]
body = "## License summary (Rust crates)\n" + "".join(
    f"- {k}: {v}\n" for k, v in counts.most_common())
body += "\n## Crates\n| Crate | Version | License |\n|---|---|---|\n" + "".join(
    f"| {n} | {v} | {l} |\n" for n, v, l in sorted(rows))
path.write_text(head + body)
print(f"{sum(counts.values())} crates, {len(counts)} license expressions")
