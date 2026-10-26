# Rulesets

Repository rules to import on GitHub: Settings › Rules › Rulesets › New ruleset › Import a ruleset. They are not applied from this folder.
The policies they enforce are in [docs/git/](../../docs/git/).

| File                | Protects  | Rules                                                                                                                                                                                                                          |
| ------------------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `main.json`         | `main`    | Only admins can update it, and only by merging a pull request. No deletion or force push. Merge commit only, 1 approval, resolved threads, up-to-date branch, green CI, `branch name` (from `dev` only) and `commit messages`. |
| `dev.json`          | `dev`     | Same rules as `main`; `branch name` requires a `<type>/<description>` source branch.                                                                                                                                           |
| `release-tags.json` | `v*` tags | Only admins can create, move or delete release tags (a tag push publishes a release, see `workflows/build.yml`).                                                                                                               |

Collaborators can push their own work branches and open pull requests, but can't merge into `dev` or `main` (the "Restrict updates" rule) nor tag a release.

After importing:

- check that the bypass list shows **Repository admin** (for `main` and `dev`: "For pull requests only"), and that the required checks are attached to **GitHub Actions**
- set `dev` as the default branch (Settings › General), so new pull requests target it
- turn on "Automatically delete head branches" (Settings › General)
- in Settings › General › Pull Requests, allow merge commits only

The required checks are job names (`check (ubuntu-latest)`, `branch name`…): update the rulesets if a job is renamed or a matrix changes.
