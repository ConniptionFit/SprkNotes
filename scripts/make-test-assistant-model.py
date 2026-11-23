#!/usr/bin/env python3
"""Writes a tiny random-weight assistant model folder (model.json + model.gguf) for tests.

It speaks gibberish, which is the point: it lets the real llama.cpp helper be exercised end to end
(load, tokenize, decode, stream, stop strings, hold, cancel) without downloading a real model.
Needs: pip install gguf numpy.   Usage: make-test-assistant-model.py <output folder> [context tokens, default 512]
"""
import hashlib
import json
import sys
from pathlib import Path

import numpy as np
from gguf import GGUFWriter, GGMLQuantizationType, TokenType

N_EMBD, N_FF, N_HEAD, N_LAYER = 64, 128, 4, 2


def vocab():
    toks, scores, types = [], [], []

    def add(t, ty, score=0.0):
        toks.append(t)
        scores.append(score)
        types.append(ty)

    add("<unk>", TokenType.UNKNOWN)
    add("<s>", TokenType.CONTROL)
    add("<|im_end|>", TokenType.CONTROL)
    add("<|im_start|>", TokenType.CONTROL)
    for b in range(256):
        add(f"<0x{b:02X}>", TokenType.BYTE)
    seen = set()
    for c in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.,:;!?-'\"()[]<>|/\n":
        if c not in seen:
            seen.add(c)
            add(c, TokenType.NORMAL, -1.0)
    add("▁", TokenType.NORMAL, -1.0)
    return toks, scores, types


def main(out_dir, ctx=512):
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(7)
    toks, scores, types = vocab()
    n_vocab = len(toks)
    path = out / "model.gguf"
    w = GGUFWriter(str(path), "llama")
    w.add_name("sprk-test-assistant")
    w.add_context_length(ctx)
    w.add_embedding_length(N_EMBD)
    w.add_block_count(N_LAYER)
    w.add_feed_forward_length(N_FF)
    w.add_head_count(N_HEAD)
    w.add_head_count_kv(N_HEAD)
    w.add_layer_norm_rms_eps(1e-5)
    w.add_rope_dimension_count(N_EMBD // N_HEAD)
    w.add_vocab_size(n_vocab)
    w.add_file_type(0)
    w.add_tokenizer_model("llama")
    w.add_token_list(toks)
    w.add_token_scores(scores)
    w.add_token_types(types)
    w.add_bos_token_id(1)
    w.add_eos_token_id(2)
    w.add_unk_token_id(0)
    w.add_add_bos_token(False)

    def mat(name, rows, cols):
        w.add_tensor(name, (rng.standard_normal((rows, cols)) * 0.05).astype(np.float32))

    def vec(name, n):
        w.add_tensor(name, np.ones(n, dtype=np.float32))

    mat("token_embd.weight", n_vocab, N_EMBD)
    vec("output_norm.weight", N_EMBD)
    mat("output.weight", n_vocab, N_EMBD)
    for i in range(N_LAYER):
        vec(f"blk.{i}.attn_norm.weight", N_EMBD)
        for n in ("attn_q", "attn_k", "attn_v", "attn_output"):
            mat(f"blk.{i}.{n}.weight", N_EMBD, N_EMBD)
        vec(f"blk.{i}.ffn_norm.weight", N_EMBD)
        mat(f"blk.{i}.ffn_gate.weight", N_FF, N_EMBD)
        mat(f"blk.{i}.ffn_up.weight", N_FF, N_EMBD)
        mat(f"blk.{i}.ffn_down.weight", N_EMBD, N_FF)
    w.write_header_to_file()
    w.write_kv_data_to_file()
    w.write_tensors_to_file()
    w.close()

    sha = hashlib.sha256(path.read_bytes()).hexdigest()
    manifest = {
        "schema": 1,
        "kind": "assistant",
        "id": "sprk-test-assistant",
        "version": "2027.01.0",
        "display_name": "Test assistant (random weights)",
        "engine": "llama-cpp",
        "min_app_version": "0.0.1",
        "files": {"weights": "model.gguf"},
        "sha256": {"model.gguf": sha},
        "assistant": {
            "tier": "titles",
            "prompt_format": "chatml",
            "context_tokens": ctx,
            "ram_estimate_mb": 50,
        },
        "license": "MIT",
        "source": "synthetic",
    }
    (out / "model.json").write_text(json.dumps(manifest, indent=2))
    print(f"wrote {out} ({path.stat().st_size} bytes)")


if __name__ == "__main__":
    if len(sys.argv) not in (2, 3):
        sys.exit(__doc__)
    main(sys.argv[1], int(sys.argv[2]) if len(sys.argv) == 3 else 512)
