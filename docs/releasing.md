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
5. **A release is a draft until it is complete.** The update check cannot see drafts, so nobody gets half a release.
6. **Rollback is "stop offering it, ship a fixed version".** See [Rollback](#rollback).

## One-time setup

Do these once. Claude cannot do them for you because they involve your account.

1. **Make a release token.** Forgejo, your avatar, Settings, Applications, new token. Name `sprknotes-release`,
   scope **write:repository** only (nothing else).
2. **Give it to CI** (only needed for the Cut release and Release workflows). Repository, Settings, Actions, Secrets, add `RELEASE_TOKEN` with that value. The workflows below use it to push tags and create releases.
3. **Uploads from the RX9070** need nothing extra: `publish-release.sh` and `ship-alpha.sh` use your git login (the one `git push` uses) when you run them yourself.
   To use a token instead, save it as the first line of `~/.config/sprknotes/forgejo-token` (`chmod 600`). Never put it in the repo.
4. **Optional, only when you start signing updates:** Forgejo refuses `.sig` attachments, and the app looks for
   `update-manifest.json.sig` by that exact name. Add `.sig` to `ALLOWED_TYPES` under `[attachment]` in the server's `app.ini`.
   Unsigned manifests are fine for pilot builds.
5. **Set your work laptop to the alpha channel:** Settings, Updates, Release channel, Alpha.

## Ship an alpha (every fix or feature batch)

Alpha is cheap on purpose. Do it as often as you want testers to have something new.

1. **Cut it.** Forgejo, Actions, **Cut release**, Run workflow, kind **alpha**. (Or `scripts/cut-release.sh alpha --push`.)
   It picks the next number (`0.2.0-alpha.3`), bumps the version, commits `chore(release): v0.2.0-alpha.3`, tags, and pushes.
2. **CI makes the draft.** The **Release** workflow checks the tag, runs the tests on that exact commit, and creates a draft
   release with notes (the CHANGELOG bullets added since the last release, plus the commit list).
3. **Build Windows and publish**, in one command on the RX9070 (from any shell, fish included):
   ```bash
   bash scripts/ship-alpha.sh --base 0.2.0 --wait-pr 80     # first alpha of 0.2.0, after PR 80 merges
   bash scripts/ship-alpha.sh                               # later alphas: next number from main
   bash scripts/ship-alpha.sh --tag v0.2.0-alpha.1          # resume a build or upload that stopped
   ```
   It checks the token file, updates `main`, cuts the tag, copies the sherpa libraries to the VM, builds that exact tag, copies `dist` back,
   uploads and publishes. Set `LIGHT_PACK_DIR=<pack folder>` to also publish the Ask Sprk model pack. The same steps by hand:
   ```bash
   git fetch --tags
   scripts/winvm.sh release-tag v0.2.0-alpha.3 -SherpaX64 <dir> -SherpaArm64 <dir>   # builds that exact tag
   scripts/winvm.sh dist                                                              # copies the dist folder to ./dist-win
   scripts/publish-release.sh all v0.2.0-alpha.3 ./dist-win --publish                 # one file per request, size-checked
   ```
   `publish` refuses to go live unless `update-manifest.json` and `SHA256SUMS.txt` are attached.
4. **Test.** Your laptop updates itself (Settings, Check for updates), or run `install.ps1 -Channel alpha`. Work through the
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
3. Build, upload and publish the Windows files exactly as in alpha step 3, using the new tag.
4. Smoke test the published stable build on a clean user profile: install, start, one short transcript, Settings, Updates shows "up to date".

New work keeps landing on `main` and ships as `0.2.1-alpha.1` and so on while 0.2.0 settles.

## Hotfix (a bug in stable)

1. `git switch -c release/0.1 v0.1.0`, push it, open a PR **into `release/0.1`** with the minimal fix (also fix it on `main`).
2. Cut it from that branch: `scripts/cut-release.sh stable --ref release/0.1 --base 0.1.1 --push` (the new tag is the tip of that branch plus the version commit).
   The Release workflow's guard is skipped for hotfixes (there is no earlier pre-release of 0.1.1), so the PR review is the gate.
3. Build, upload, publish as above. Users on stable get it at their next update check.

## Rollback

Installed copies are never downgraded, so rollback has two parts:

1. **Stop offering the bad build.** `scripts/publish-release.sh yank v0.2.0` turns it back into a draft. New installs and update checks no longer see it, and
   clients fall back to the previous release on their channel. (Same effect from the Forgejo release page: edit, tick Draft.)
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
| Draft, upload, publish, yank | `scripts/publish-release.sh` (token from `RELEASE_TOKEN` or `~/.config/sprknotes/forgejo-token`) |
| Verify and draft on tag push | `.forgejo/workflows/release.yml` |
| Windows build | `scripts/release.ps1` via `scripts/winvm.sh release-tag` |
| Update manifest, channels | `docs/deployment.md`, ADR-011 |

Forgejo limits to remember: 2 GB per file, 5 files per request (the script sends one per request), only some file types
(others get a `.zip` suffix, which the update manifest records), and Cloudflare rejects a single upload over about 100 MB.
The current installers are well under that; a bundled-model build is not hosted (use the Download model button).

## Later, when you want less hand work

- **Windows builds in CI.** Install the Forgejo runner on `sprk-winvm` with the label `windows`, then add a job to `release.yml` that runs
  `scripts/release.ps1` and uploads with `publish-release.sh`. Not done now because the VM needs the sherpa libraries, LLVM and WiX set up as a
  service account first, and update-manifest signing needs the 1Password agent, which is not available to a headless runner.
- **Linux package in CI.** The Linux tarball is still built by hand; a CI job needs the Tauri system libraries on the runner.
- **Signed manifests.** Required from 1.0 (hard rule 14 and ADR-011).
- **GitHub.** `.github/workflows/ci.yml` mirrors only the checks. If the repository moves to GitHub, port `release.yml` and `cut-release.yml`
  (same steps; use `GITHUB_TOKEN` with `contents: write`).
