# ADR-010: Assistant model packs

Status: **accepted** (2026-11-23). Extends SPEC 16 and ADR-005. Companion to ADR-009.

## Context
Assistant models are large weight files (GGUF), not speech models. They must arrive the same safe way (`.sprkmodel`, verified hashes, allow-list, signing from 1.0) without being mistaken for transcription models or running code (hard rule 9).

## Decision
- `model.json` schema stays **1**. New optional field **`kind`**: `transcription` (default when absent, so every existing pack is unchanged) or `assistant`. A pack with no `kind` serializes without one.
- An assistant pack carries an **`assistant`** section: `tier` (`titles`, `light`, `standard`), `prompt_format` (a plain id such as `chatml` that the app maps to a built-in format), `context_tokens`, `ram_estimate_mb`, `attribution`. It needs no `languages`, `sample_rate` or `features`.
- **A pack never carries a chat template or any code.** The app owns prompt formats (`sprk_assistant::prompt`); an unknown id is refused. The GGUF's embedded template is ignored.
- **Separate stores.** A `ModelStore` accepts one kind (`ModelStore::for_kind`); importing the wrong kind is rejected with a plain reason and nothing is installed. The assistant has its own folders and its own `active.json`, so the transcription resolver never sees assistant packs and the other way round. Hard rule 10 applies to the transcription model; the assistant model is chosen when a session starts and is not swapped while it runs.
- Large packs use the multipart release flow from ADR-005. The assistant is never bundled in the installer; it is a download or an admin deployment (`sprkctl`, `Install-SprkModel.ps1`).
- Licenses with notice requirements (for example Apache 2.0) put the text in `attribution`; the app shows it in the notices.

## Consequences
- `Manifest` gained `kind` and `assistant`; validation is per kind. `sprkctl` and the resolver need no change for transcription packs.
- The next steps (helper, resolver wiring for the assistant store, Settings row, download index entries) build on this.
