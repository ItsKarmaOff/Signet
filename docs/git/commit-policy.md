# Commit Policy

Commits follow [Conventional Commits](https://www.conventionalcommits.org/).

## Format

```
<type>(<scope>): <short description>

[optional body]

[optional footer]
```

## Types

| Type         | Description                                                 |
| ------------ | ----------------------------------------------------------- |
| **feat**     | Adding a new feature                                        |
| **fix**      | Bug fix                                                     |
| **docs**     | Documentation changes or additions                          |
| **style**    | Formatting changes with no impact on logic                  |
| **refactor** | Code restructuring without adding features or fixes         |
| **perf**     | Performance improvements                                    |
| **test**     | Adding or modifying tests                                   |
| **build**    | Build system, packaging or dependencies                     |
| **ci**       | Continuous integration                                      |
| **merge**    | Merge commits (see the [pull request policy](pr-policy.md)) |
| **revert**   | Reverting a previous commit                                 |
| **security** | Security fixes                                              |
| **chore**    | Maintenance tasks (rarely used, prefer the types above)     |

Prefer the most specific type: `chore` only when nothing else fits.

| Change                                      | Type       |
| ------------------------------------------- | ---------- |
| Only tests (`*.test.ts`, `tests/e2e/`)      | `test`     |
| Only README, `docs/`, CONTRIBUTING          | `docs`     |
| Dependencies, electron-builder, Expo config | `build`    |
| `.github/workflows/`, rulesets              | `ci`       |
| Prettier, lint fixes                        | `style`    |
| Structure without new behavior              | `refactor` |
| Version bump before a release               | `chore`    |

## Scope

Optional but recommended: the area touched.

| Scope                                                       | Area                        |
| ----------------------------------------------------------- | --------------------------- |
| `desktop`, `mobile`, `shared`                               | A whole package             |
| `tabs`, `spaces`, `passwords`, `adblock`, `reader`, `link`… | A desktop or mobile feature |
| `ui`                                                        | Interface, styles           |
| `ipc`                                                       | The typed IPC contract      |
| `i18n`                                                      | Translations                |
| `macos`, `linux`, `windows`, `android`, `ios`               | A single system             |
| `deps`                                                      | Dependencies                |
| `release`                                                   | Versions and releases       |

## Description

- **Imperative mood**, as if completing "This commit will…": `add`, `fix`, `update`
- **Starts with a lowercase letter or a digit**
- **No period at the end**
- **Under 72 characters**
- **In English**
- From the user's point of view when the change is visible: say what changes for them, not which function moved

**Good:**

```
feat(tabs): make the tab sleep delay configurable
fix(mobile): open the page and tab menus without bouncing
refactor(ipc): check arguments in one typed contract
chore(release): bump version to 0.2.0
```

**Bad:**

```
added tab groups (past tense, no type)
fix(tabs): Fix the scroll. (uppercase, period)
feat: mise à jour des onglets (French)
```

## Body and Footer

- The body explains **why**, and the impacts; separate paragraphs with a blank line
- The footer references issues (`Closes #42`) and breaking changes (`BREAKING CHANGE: …`)
- No AI attribution or `Co-Authored-By` trailer for tools

## Atomic Commits

- **One commit = one logical intent**: don't mix a refactor and a feature
- Each commit can be reverted on its own
- Run the checks before committing (see CONTRIBUTING)
- Never commit build output (`out/`, `dist/`), `node_modules/`, generated native projects (`app/mobile/ios/`, `app/mobile/android/`) or local files (`.env.local`, `.DS_Store`)

## Enforcement

The `commit messages` check (`.github/workflows/commit-policy.yml`) runs on every pull request and refuses any commit (merge commits aside) whose subject doesn't match this format.
