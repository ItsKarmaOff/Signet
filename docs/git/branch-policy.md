# Branch Policy

This document defines how branches are named and where they go.

## Long-lived Branches

| Branch | Role                                                                   | Updated by                                              |
| ------ | ---------------------------------------------------------------------- | ------------------------------------------------------- |
| `dev`  | Integration branch: every change lands here first. The default branch. | Pull requests from work branches, merged by maintainers |
| `main` | What is released. Release tags are cut from it.                        | Pull requests from `dev` only, merged by maintainers    |

```
<type>/<short-description> --PR--> dev --PR--> main --tag--> vX.Y.Z
```

- Every work branch is created from `dev` and merged back into `dev`
- `main` only receives `dev`, when a release is prepared (see the [release policy](release-policy.md))
- Nobody pushes directly to `dev` or `main`, maintainers included: every change goes through a pull request
- There is no hotfix path straight to `main`: an urgent fix is a `fix/…` branch into `dev`, followed by a release

## Branch Format

Each work branch MUST follow this format:

```
<type>/<short-description>
```

- **Lowercase** only
- **Hyphens** (`-`) between words
- **Short and descriptive** (2-4 words, under 50 characters in total)
- **In English**
- No uppercase, underscores or special characters

## Branch Types

| Type         | Description                                             |
| ------------ | ------------------------------------------------------- |
| **feat**     | Adding a new feature                                    |
| **fix**      | Bug fix                                                 |
| **docs**     | Documentation changes or additions                      |
| **style**    | Formatting changes with no impact on logic              |
| **refactor** | Code restructuring without adding features or fixes     |
| **perf**     | Performance improvements                                |
| **test**     | Adding or modifying tests                               |
| **build**    | Build system, packaging or dependencies                 |
| **ci**       | Continuous integration                                  |
| **security** | Security fixes                                          |
| **chore**    | Maintenance tasks (rarely used, prefer the types above) |

**Good:**

```
feat/tab-groups
fix/peek-scroll
docs/linux-install
refactor/ipc-contract
chore/release-0.2.0
```

**Bad:**

```
feat/TabGroups (uppercase)
fix/peek_scroll (underscores)
feature/tab-groups (wrong type)
fix/123 (issue number alone)
alex/fix (personal name, no type)
fix/stuff (vague)
```

## Rules

- **One branch = one logical intent**, mapped to a single pull request
- **Keep it up to date** with `dev` (merge `dev` into it, or rebase before review)
- **Delete it** after merge (GitHub does it automatically)
- Contributors without write access work on a fork, with the same naming

## Enforcement

The `branch name` check (`.github/workflows/branch-policy.yml`) runs on every pull request and refuses:

- a pull request into `main` from anything other than `dev` of this repository
- a pull request into `dev` from a branch outside the format above (Dependabot's `dependabot/…` branches are accepted)
- a pull request into any other branch than `main`, `dev` or a work branch

The rulesets in `.github/rulesets/` protect `main` and `dev` on GitHub: no direct push, no force push, no deletion, and only maintainers can merge.
