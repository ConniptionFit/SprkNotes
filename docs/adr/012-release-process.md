# ADR-012: Release process (alpha, beta, stable)

Status: **accepted** (owner request, 2026-11-23). Operating guide: [releasing.md](../releasing.md).

## Context
Fixes need to reach a few testers fast and reach everyone only after they hold up. ADR-011 already gives the app channels read from
the tag. Missing was a repeatable way to get a build from a commit to a channel, a rule for promotion, and a way to go back. One maintainer, one
Windows build VM, a Forgejo server that limits attachments, and an update manifest that is signed per build.

## Decision
1. **Trunk-based.** `main` is always releasable. Alphas are cut from `main`. Beta and stable are cut from a release branch
   (`release/X.Y.Z`) that starts at the tested tag. Hotfixes use `release/X.Y` branched from the old stable tag.
2. **SemVer tags** `vX.Y.Z-alpha.N`, `vX.Y.Z-beta.N`, `vX.Y.Z`. `scripts/release-version.sh` is the only place numbers are computed.
3. **Build once per version, promote the tested source.** The version string is compiled into the app and installers and is covered by the signed manifest, so a
   build cannot change channel without being rebuilt. Instead a promotion is the tested tag plus one commit touching only version files, enforced by
   `release-version.sh guard` in CI. Rebuild is the same locked source with a new version number.
4. **Draft first.** CI creates a draft release on every tag after tests pass; Windows files are added next; `publish` refuses without
   `update-manifest.json` and `SHA256SUMS.txt`. The updater ignores drafts, so a half-uploaded release is never offered.
5. **Scoped token, not a person's credential.** Tag pushes and release API calls use a `write:repository` token (`RELEASE_TOKEN`), not a stored git login.
6. **Rollback is yank plus roll forward.** The updater never downgrades, so a bad release becomes a draft and a higher version is published.
7. **Notes from the CHANGELOG diff** between tags, with commit subjects as a fallback.

## Not decided here
Windows builds in CI (needs a runner on the VM), Linux packaging in CI, and signing keys for unattended signing. Listed in releasing.md.

## Consequences
One extra bump commit per release. The Windows files still come from the VM, started by one command and uploaded with one command.
