# Release Policy

Releases are made by maintainers only.

## Versioning

Signet follows **Semantic Versioning**: `MAJOR.MINOR.PATCH`.

| Change                                                   | Bump      |
| -------------------------------------------------------- | --------- |
| Breaking change (profile data, settings, link protocol…) | **MAJOR** |
| New feature                                              | **MINOR** |
| Bug fix, performance, security fix, dependency update    | **PATCH** |

Prereleases add a suffix and are published as GitHub prereleases:

| Suffix   | Meaning                                | Example        |
| -------- | -------------------------------------- | -------------- |
| `beta.N` | A snapshot of `main`, may contain bugs | `0.2.0-beta.1` |
| `rc.N`   | Release candidate, final testing       | `0.2.0-rc.1`   |

A new version is strictly greater than every version already tagged (`0.2.0-rc.3` < `0.2.0`).

## Where the Version Lives

The version is written in five files, all with the same value:

- `package.json`, `app/desktop/package.json`, `app/mobile/package.json`, `packages/shared/package.json`
- `app/mobile/app.json` (`expo.version`)

`scripts/check-version.mjs` checks them; the release script and the Build workflow both run it.

## Tags

- Format: `vX.Y.Z` or `vX.Y.Z-beta.N` / `vX.Y.Z-rc.N`, **with the `v` prefix**
- Annotated (`Signet X.Y.Z`), cut from `main`, created by `scripts/release.sh` only
- Only maintainers can create, move or delete a `v*` tag (ruleset `release-tags`): a tag is a publication
- Never move or delete a published tag: release a new version instead

## Release Process

1. **Bump**: a branch `chore/release-X.Y.Z` from `dev` updates the five files and `package-lock.json` (`npm install`), commit `chore(release): bump version to X.Y.Z`, pull request into `dev`, merged as `merge "chore/release-X.Y.Z" into "dev"`.
2. **Promote**: a pull request from `dev` into `main` titled `chore(release): X.Y.Z`, with all checks green, merged as `merge "dev" into "main"`.
3. **Tag**: on an up-to-date `main`:

   ```bash
   git switch main && git pull
   npm run release -- X.Y.Z            # or --dry-run to only run the checks
   ```

   The script refuses to run off `main`, with uncommitted changes, out of sync with `origin`, with a version that doesn't match the five files, that already exists or isn't greater than the last tag. It then creates and pushes the tag.

4. **Publish**: the tag starts the Build workflow. It checks the tag against the files again, builds macOS (`.dmg`, Apple Silicon and Intel), Linux (AppImage, `deb`, `rpm`, x64 and ARM), Windows (installer) and Android (APK), then publishes the GitHub release. Its notes list what changed since the previous release, built from the commit messages by `scripts/release-notes.sh` (`feat`, `fix`, `perf` and `security` commits only), followed by a table of which file to download for each system. `npm run release -- <version> --dry-run` shows a preview. It refuses to publish if the tag was pushed by someone who isn't an admin, and publishes nothing if one build fails.

Builds are not signed yet: on macOS, right-click › Open the first time; on Windows, SmartScreen warns.

## Releases to Avoid

- A tag created, moved or deleted by hand
- A tag cut from another branch than `main`
- A release with failing checks
- Skipped version numbers (`0.2.0` → `0.5.0`)
- Releasing on a Friday, unless it's an urgent fix
