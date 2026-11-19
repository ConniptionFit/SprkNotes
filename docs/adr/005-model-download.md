# ADR-005: Model download and update check

Status: **accepted** (owner request, 2026-11-19). Amends hard rule 2 ("No network", "no update checks").

## Context
A portable zip that carries a 660 MB model is heavy to publish and to copy. The owner asked for a
lightweight zip with a "Download model" button in Settings that always fetches the latest Parakeet and
checks for a newer model when the app starts, while keeping a bundled-model build for offline use.
SprkNotes otherwise makes no outbound connections (HIPAA posture).

## Decision
SprkNotes may make https requests for **model packs only**:
1. The model index (`models-index.json`, under 256 KB) and the `.sprkmodel` packs it lists.
2. A silent index check at launch. It only tells the user a newer model exists; it never downloads on its own.
3. Downloads happen only when the user presses "Download model" (never during a session).

Guards:
- https only, redirects to http refused, response sizes capped. WinHTTP is used so the system proxy and
  certificate store (company CAs) apply. No third-party TLS stack ships in the app.
- Nothing about the user, the PC or any transcript is sent. The requests are plain GETs; the index host sees an
  IP address and a user agent of `SprkNotes`.
- Each pack must match the SHA-256 in the index, then pass the normal import rules: manifest file hashes,
  engine compatibility, smoke test, and the Ed25519 signature (mandatory from 1.0). A failed pack changes nothing.
  The index is not trusted to vouch for a pack on its own; the 1.0 signature is what stops a hostile mirror.
- Admin registry policy: `ModelDownload` (0 turns the button and all network use off), `ModelUpdateCheck`
  (0 turns off the launch check only), `ModelIndexUrl` (https address of an internal mirror). `PinnedModel`,
  `AllowedModelHashes`, `RequireSignedModels` and `ModelPublicKeys` keep applying to downloaded packs.
- A build with the model bundled still works fully offline.

Default index: the repository's `models` release (`models-index.json` plus the packs). Maintainers publish a new
model by packing it, running `sprkctl model index`, and replacing the assets on that release.

## Consequences
- CLAUDE.md rule 2, SPEC 7 (network row), 12 and the privacy text change: the only allowed outbound traffic is the
  model index and pack downloads described here. Telemetry, analytics, crash upload and any other update check stay forbidden.
- Firewall allow-lists need the index host (or the internal mirror).
- Regulated sites can turn it all off with `ModelDownload=0` and keep using the bundled build or `Install-SprkModel.ps1`.
