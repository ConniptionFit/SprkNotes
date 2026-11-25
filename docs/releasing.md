# Releasing SprkNotes

How a change gets from a commit to your users: **alpha, then beta, then stable**. One person can run it in a few
minutes. Design reasons are in [ADR-012](adr/012-release-process.md); how the app finds releases is in
[deployment.md](deployment.md#updates-channels-and-locking).

## The idea in one picture

```
 PR merged to main ──► alpha  v0.2.0-alpha.1, .2, .3 ...   (you and testers, any time)
                          │  confident? promote the SAME code
                          ▼
                        beta   v0.2.0-beta.1               (pilot group, optional)
                          │
                          ▼
                        stable v0.2.0                      (everyone)
```

| Channel | Tag | Who gets it | Cut from |
|---|---|---|---|
| alpha | `v0.2.0-alpha.N` | Installs set to **Alpha** (your work laptop) | `main` |
| beta | `v0.2.0-beta.N` | Installs set to **Beta** or **Alpha** | the alpha you tested |
| stable | `v0.2.0` | Everyone | the beta (or alpha) you tested |

A channel includes the more stable ones, and the app never downgrades. Pick a channel in Settings, **Updates**
(or by policy, see deployment.md).

## Rules

1. **`main` always builds.** Every change reaches `main` by pull request with green checks (fmt, clippy, tests, terms).
2. **Every fix goes to alpha first.** Nothing is released straight to stable.
3. **Promote the tested code, do not re-decide.** A beta or stable tag is the alpha you tested plus one commit that changes only
   version files (`Cargo.toml`, `Cargo.lock`, `app/ui/package.json`, `CHANGELOG.md`). The release workflow fails if anything else differs.
4. **Version numbers are SemVer** and come only from the root `Cargo.toml` (hard rule 7). Tags are `v` plus the version.
   Patch (`0.2.1`) for fixes, minor (`0.3.0`) for features, major only for breaking changes.
   In practice: a version is tested as numbered alphas (`0.2.0-alpha.1`, `.2`, `.3` ...) until it is called done, then
   released as plain `0.2.0`. The next feature batch starts at `0.3.0-alpha.1`; a fix-only release after `0.2.0` is `0.2.1`
   (alpha first). Numbers only go up: never reuse, skip back or restart a number within a version.
5. **A release is a draft until it is complete.** The update check cannot see drafts, so nobody gets half a release.
6. **Rollback is "stop offering it, ship a fixed version".** See [Rollback](#rollback).

## One-time setup

Nothing to create: releases are built and published by GitHub Actions with the built-in `GITHUB_TOKEN`.

1. **Allow Actions to write.** Repository, Settings, Actions, General, Workflow permissions: **Read and write permissions**.
2. **Set your work laptop to the alpha channel:** Settings, Updates, Release channel, Alpha.
3. **Optional, only when you start signing updates:** the signing key lives in your 1Password agent, which a hosted runner cannot reach, so
   until then manifests are unsigned (fine for pilot builds). Signing is a 1.0 gate (hard rule 14).

## Ship an alpha (every fix or feature batch)

Alpha is cheap on purpose. Do it as often as you want testers to have something new.

1. **Cut it.** GitHub, Actions, **Cut release**, Run workflow, kind **alpha**. (Or `scripts/cut-release.sh alpha --push`, which also starts the
   Release workflow because the tag push does.) It picks the next number (`0.2.0-alpha.3`), bumps the version, commits
   `chore(release): v0.2.0-alpha.3`, tags, and pushes.
2. **CI does the rest.** The **Release** workflow checks the tag, runs the tests on that exact commit, builds the x64 and ARM64 apps, MSIs, MSIX
   bundle, portable zip and update manifest natively on Windows runners (`scripts/release.ps1`), creates a draft release with notes (the
   CHANGELOG bullets added since the last release, plus the commit list) and publishes it. `publish` refuses to go live unless
   `update-manifest.json` and `SHA256SUMS.txt` are attached. A failed run can be re-run, or started again from Actions, **Release**, with the tag.
3. **Test.** Your laptop updates itself (Settings, Check for updates), or run `install.ps1 -Channel alpha`. Work through the
   checks in [manual-checks.md](manual-checks.md) that the change touches.

Add a CHANGELOG bullet under `## Unreleased` in the same PR as the change. That is what the release notes are made from.

## Promote (when you trust it)

Promotion does not rebuild a different thing: it re-cuts **the same commit** with a new version, and CI proves nothing else changed.
(The version is compiled into the app and the installers, so a byte-identical file cannot change channel. Same source,
same locked dependencies, one version-bump commit is the closest real-world equivalent.)

**Checklist before promoting**
- [ ] The alpha has run on your laptop for the time you set (suggestion: a few real meetings, no new bugs).
- [ ] `scripts/winvm.sh test` passes on the VM, and the checks in manual-checks.md for the changes are ticked.
- [ ] No open bug marked "blocks release".
- [ ] You name the exact alpha you tested, for example `v0.2.0-alpha.3`.

**Do it** (same workflow, so you pick the build explicitly):
1. Actions, **Cut release**, kind **beta** (or **stable**), `from` = the alpha you tested. The branch `release/0.2.0` is created from that tag,
   one version commit is added, and the new tag is pushed. Skipping beta is fine: promote straight to stable.
2. CI checks that the new tag equals the tested tag plus version files only, then drafts the release with notes covering everything since the last
   release of that channel (stable notes cover everything since the previous stable, so people who skip pre-releases see it all).
3. The Release workflow builds and publishes the Windows files exactly as for an alpha.
4. Smoke test the published stable build on a clean user profile: install, start, one short transcript, Settings, Updates shows "up to date".

New work keeps landing on `main` and ships as `0.2.1-alpha.1` and so on while 0.2.0 settles.

## Hotfix (a bug in stable)

1. `git switch -c release/0.1 v0.1.0`, push it, open a PR **into `release/0.1`** with the minimal fix (also fix it on `main`).
2. Cut it from that branch: `scripts/cut-release.sh stable --ref release/0.1 --base 0.1.1 --push` (the new tag is the tip of that branch plus the version commit).
   The Release workflow's guard is skipped for hotfixes (there is no earlier pre-release of 0.1.1), so the PR review is the gate.
3. The Release workflow builds and publishes it. Users on stable get it at their next update check.

## Rollback

Installed copies are never downgraded, so rollback has two parts:

1. **Stop offering the bad build.** `scripts/publish-release.sh yank v0.2.0` turns it back into a draft. New installs and update checks no longer see it, and
   clients fall back to the previous release on their channel. (Same effect from the GitHub release page: edit the release and choose "Save draft".)
2. **Ship a fix.** People who already updated stay on the bad build until a higher version exists, so publish `0.2.1` (hotfix) or the next alpha
   as soon as the fix is ready. A revert commit is a normal fix.

Never delete a tag or re-upload files to a published version: checksums in `update-manifest.json` would no longer match.
(A **draft** can be re-uploaded freely; `publish-release.sh upload` replaces same-named files.)

## Where things live

| Thing | Where |
|---|---|
| Version rules, next number, promotion guard | `scripts/release-version.sh` |
| Bump, commit, tag, push | `scripts/cut-release.sh` (the "Cut release" workflow runs it) |
| Release notes | `scripts/release-notes.sh <tag>` |
| Draft, upload, publish, yank | `scripts/publish-release.sh` (GitHub: the `gh` CLI, `GH_TOKEN` in CI; Forgejo: the same script) |
| Verify and draft on tag push | `.github/workflows/release.yml` |
| Windows build | `scripts/release.ps1`, run by the Release workflow (or by hand on a Windows PC) |
| Update manifest, channels | `docs/deployment.md`, ADR-011 |

GitHub limits to remember: 2 GB per file. File names are published as they are (no suffix tricks).

## Later, when you want less hand work

- **Linux package in CI.** The Linux tarball is still built by hand; a CI job needs the Tauri system libraries on the runner.
- **Signed manifests.** Required from 1.0 (hard rule 14 and ADR-011); needs a signing step that does not depend on the 1Password agent on a laptop.
