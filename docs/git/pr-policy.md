# Pull Request Policy

## Who Does What

Signet is open source: anyone can propose a change. Merging and releasing are reserved to maintainers.

| Role                               | Can                                                                         | Cannot                                    |
| ---------------------------------- | --------------------------------------------------------------------------- | ----------------------------------------- |
| **Contributor** (anyone)           | Fork, open pull requests into `dev`, comment, review                        | Push to this repository                   |
| **Collaborator** (write or triage) | Also push work branches to this repository, triage issues and pull requests | Merge into `dev` or `main`, tag a release |
| **Maintainer** (repository admin)  | Merge pull requests, open the `dev` → `main` pull request, release          | Push directly to `dev` or `main`          |

GitHub enforces this with the rulesets in `.github/rulesets/`: only admins can update `dev`, `main` and `v*` tags, and only through a pull request for the branches.

## Title

```
<type>(<scope>): <short description>
```

Same rules as the [commit policy](commit-policy.md): imperative mood, lowercase start, no period, under 72 characters, **in English**. Titles become the release notes, so write them for the people using Signet.

```
feat(tabs): add tab groups
fix(link): reconnect the phone after the computer wakes up
chore(release): 0.2.0
```

The pull request from `dev` into `main` is titled `chore(release): <version>`.

## Description

Follow the template (`.github/pull_request_template.md`): what changes and why, the linked issue (`Closes #123`), how it was tested and on which systems, screenshots for interface changes. The description can be written in English or French.

## Rules

### Scope and Size

- **One pull request = one branch = one logical intent**
- Keep it small (under 400 changed lines when possible); split a large feature into steps
- Name the branch after the [branch policy](branch-policy.md)

### Target

- Work branches target `dev`
- Only `dev` targets `main`, opened by a maintainer to prepare a release
- A pull request may target another work branch to stack changes; it reaches `dev` through that branch

### Checks and Review

- Every CI check must pass: types, lint, format, translations, unit and end-to-end tests, mobile and Android builds, `branch name`, `commit messages`
- The branch must be up to date with its target
- At least **one approval** from someone other than the author; a maintainer merging their own pull request alone bypasses this knowingly
- All review threads resolved before merge
- Re-request a review after significant changes

## Merging

Maintainers merge with a **merge commit**, never squash or rebase, so each change keeps its history. The merge commit message is:

```
merge "<source-branch>" into "<target-branch>"
```

```
merge "feat/tab-groups" into "dev"
merge "fix/peek-scroll" into "dev"
merge "dev" into "main"
```

- Branch names in double quotes, exactly as in Git (a fork's branch: its name without the owner)
- Lowercase `merge`
- Replace GitHub's default message ("Merge pull request #…") when merging
- The source branch is deleted after merge

## Lifecycle

1. **Branch** from `dev`, following the [branch policy](branch-policy.md)
2. **Commit** following the [commit policy](commit-policy.md)
3. **Open** the pull request into `dev`, with a clear title and description
4. **Review**: address feedback, push fixes, re-request a review
5. **Merge**: a maintainer merges once approved and green
6. **Clean up**: the branch is deleted

## Pull Requests to Avoid

- No description, or a vague title (`fix stuff`, `update`, `wip`)
- Unrelated changes mixed together
- The wrong target (`main` instead of `dev`)
- A draft left open for weeks without activity
