# Moving or forking SprkNotes

SprkNotes does not depend on the host, owner or name of the repository it lives in. Moving it to another
Forgejo or GitHub repository (for example a corporate GitHub organization), or forking it into a
separately branded edition, is a short checklist.

## What names the repository

| Where | What | Change when moving |
|---|---|---|
| `distribution.toml` | **The one source.** Repository address, provider, tag that carries `models-index.json`. Compiled into the app as the default update and model source. | Yes |
| `install.ps1` | Default `-RepoUrl` (a piped script cannot read the toml). | Yes, one line |
| `README.md`, `docs/install.md` | The one-line install command. | Yes |
| `.forgejo/workflows/ci.yml` and `.github/workflows/ci.yml` | Same jobs for either host. Forgejo uses its own folder when both exist. `scripts/check-ci-parity.sh` keeps them identical. | No, delete the one you do not use |

Everything else (updater, model download, policy, MSI switches, Intune and NinjaOne scripts) reads the
address from the build default, a user choice, or admin policy. Admins can point any install at another
repository without a rebuild: `UpdateRepoUrl` (+ `UpdateProvider`) and `ModelIndexUrl`, or the MSI switches in
[deployment.md](deployment.md).

A build can also override the file: `SPRK_REPO_URL=https://github.com/acme/sprknotes SPRK_REPO_PROVIDER=github cargo build`.

## Move to a new repository (same product)

1. Create the repository and push the history (`git push --mirror`).
2. Edit `distribution.toml` (`repo_url`; set `provider = "github"` only for GitHub Enterprise Server or an unusual host).
3. Edit the default in `install.ps1` and the install command in `README.md` and `docs/install.md`.
   On GitHub the raw script address is `https://raw.githubusercontent.com/OWNER/REPO/main/install.ps1`.
4. Delete the workflow folder for the host you left. On GitHub, add the repository secrets your signing step needs.
5. Publish releases the same way on the new host: tags like `v1.2.3` or `v1.3.0-beta.1`, each release carrying
   `update-manifest.json` plus the installers; one release tagged `models` carrying `models-index.json` and packs.
6. Installs already out there keep looking at the old address until they update once from it, or an admin sets
   `UpdateRepoUrl`. Publish one last release on the old host whose build default is the new address.

GitHub notes: release assets can be MSI files directly (Forgejo needs the `.zip` suffix trick); file size limit is
2 GB per asset. Allow `github.com`, `api.github.com` and `objects.githubusercontent.com` through firewalls.

## Fork into a separate edition (own name, own installs)

Do the steps above, then change what makes this a *different product*:

| What | Where | Why |
|---|---|---|
| App identifier | `app/src-tauri/tauri.conf.json` (`identifier`) | Keeps settings and data of the two editions apart. |
| Product name and window titles | `app/src-tauri/tauri.conf.json`, `packaging/wix/Package.wxs` (`Name`, `Manufacturer`), `packaging/msix`, `packaging/linux` | What users and Add/Remove Programs see. |
| **MSI `UpgradeCode`** | `packaging/wix/Package.wxs` | **Generate a new GUID for a fork** so the two editions can be installed side by side and never upgrade each other. **Keep it unchanged when merely moving the same product**, or existing installs would not upgrade in place. |
| Policy registry key and ADMX | `crates/sprk-config`, `packaging/admx` | Only if the edition must not share policy with the original. |
| Update signing keys | `UpdatePublicKeys` policy, `BUILTIN_UPDATE_KEYS` in `app/src-tauri/src/updater.rs` | Use your own release key. |
| Icons and brand colors | `app/src-tauri/icons`, `assets`, `app/ui/src` | Branding. |
| Licenses | `LICENSE`, `THIRD_PARTY_NOTICES.md` (`scripts/gen-notices.py`) | Keep the third-party notices (Parakeet is CC-BY-4.0 and needs attribution). |

Check before the first release: `scripts/check-terms.sh`, `scripts/check-ci-parity.sh`, then
`grep -rn "<old-host>\|<old-owner>" . --exclude-dir=.git` should only find history in `CHANGELOG.md` and ADRs.
