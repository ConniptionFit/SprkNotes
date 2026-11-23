#!/usr/bin/env python3
"""Makes an assistant model folder (model.json + the GGUF file) ready for `sprkctl model pack`.

  make-assistant-pack-folder.py <weights.gguf> <output folder> --id qwen3-5-2b-q4 --version 2027.01.0 \
      --tier light --prompt-format chatml-nothink --context 8192 --ram-mb 2200 \
      --license Apache-2.0 --source "<where the weights came from>" --attribution "<notice text>" \
      --display-name "Qwen3.5 2B (Q4)"

The weights are copied (or hard-linked when possible) into the folder as model.gguf. Prompt format
ids are the ones built into the app (chatml, chatml-nothink).
"""
import argparse
import hashlib
import json
import os
import shutil
from pathlib import Path


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("weights")
    ap.add_argument("out")
    ap.add_argument("--id", required=True)
    ap.add_argument("--version", required=True, help="YYYY.MM.patch")
    ap.add_argument("--tier", required=True, choices=["titles", "light", "standard"])
    ap.add_argument("--prompt-format", default="chatml-nothink")
    ap.add_argument("--context", type=int, default=8192)
    ap.add_argument("--ram-mb", type=int, required=True)
    ap.add_argument("--license", required=True)
    ap.add_argument("--source", required=True)
    ap.add_argument("--attribution", default="")
    ap.add_argument("--display-name", required=True)
    ap.add_argument("--min-app-version", default="0.0.1")
    a = ap.parse_args()

    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    dest = out / "model.gguf"
    if dest.exists():
        dest.unlink()
    try:
        os.link(a.weights, dest)
    except OSError:
        shutil.copyfile(a.weights, dest)
    h = hashlib.sha256()
    with open(dest, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    manifest = {
        "schema": 1,
        "kind": "assistant",
        "id": a.id,
        "version": a.version,
        "display_name": a.display_name,
        "engine": "llama-cpp",
        "min_app_version": a.min_app_version,
        "files": {"weights": "model.gguf"},
        "sha256": {"model.gguf": h.hexdigest()},
        "assistant": {
            "tier": a.tier,
            "prompt_format": a.prompt_format,
            "context_tokens": a.context,
            "ram_estimate_mb": a.ram_mb,
            "attribution": a.attribution,
        },
        "license": a.license,
        "source": a.source,
    }
    (out / "model.json").write_text(json.dumps(manifest, indent=2))
    print(f"wrote {out}/model.json for {a.id}@{a.version} ({dest.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
