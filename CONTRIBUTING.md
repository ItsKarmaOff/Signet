# Contributing to Signet

Thanks for your interest in Signet. Bug reports, fixes, features, translations and docs are all welcome. This guide covers how to set up the project, how the code is organized, and what a pull request needs before it can be merged.

**Language.** Issues and pull requests can be written in English or French. Code comments, logs and commit messages are in English. The in-depth docs (`docs/`) are still in French for now.

## Ways to contribute

- **Report a bug**: open an [issue](https://github.com/ItsKarmaOff/Signet/issues) with the details listed in [Reporting bugs](#reporting-bugs).
- **Suggest a feature**: open an issue describing the problem you want solved, not only the solution. Small, focused ideas are easier to land.
- **Fix something**: issues with no one assigned are open. Comment on the issue first so two people don't work on the same thing.
- **Translate**: the interface is in English and French. Fixing wording in either language helps, and so does proposing a new language (open an issue first, it touches the language picker).
- **Improve the docs**: including translating `docs/` to English.

For anything larger than a bug fix (a new feature, a refactor, a new dependency), **open an issue before writing code**, so we can agree on the approach before you spend time on it.

Security issues don't go in public issues: see [SECURITY.md](SECURITY.md).

Everyone taking part in the project, in issues, pull requests and discussions, is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Setting up

You need [Node.js](https://nodejs.org/) 24 (the version CI uses), npm and git.

```bash
git clone https://github.com/<your-username>/Signet.git
cd Signet
npm install
npm run dev
```

One `npm install` at the root sets up the three workspaces. If the app doesn't start because Electron's binary is missing, run `node node_modules/electron/install.js`.

For the mobile app, `npm run mobile` starts Expo; see [app/mobile/README.md](app/mobile/README.md) for Expo Go, simulators, Android and native builds.

**Don't build while `npm run dev` is running.** Running `electron-vite build` (or `npm run build`) into `out/` during dev rewrites files the running app reads, and the orphaned Electron process can no longer quit. End-to-end tests build into `out-e2e/` for this reason.

## Finding your way in the code

Start with [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): it's the map of the code. In short:

```
app/desktop/src/
  main/       main process (Node.js): windows, tabs, state, disk
    model/    tab and space rules, with no Electron import (testable on their own)
    browser/  the Browser class and its parts: views, layout, overlay, Peek…
    features/ one file or folder per feature: passwords, notes, extensions, backup…
    ipc/      handlers for calls from the interface, one file per domain
  preload/    scripts that run inside pages before them
  renderer/   the React interface (sidebar, command bar, settings…)
  shared/     types and the IPC contract shared by the desktop processes (alias @shared)
app/mobile/   Expo app: routes in app/, screens in src/features/<feature>/
packages/shared/  platform-neutral TypeScript used by desktop and mobile
```

Where new code goes:

- **A rule both apps can use** (data model, URL handling, tree operations…) goes in `packages/shared`, as pure functions with a test next to them. It must not import Electron, `node:*`, the DOM or React Native; the type check fails if it does. See [packages/shared/README.md](packages/shared/README.md).
- **Code shared by the desktop processes only** (IPC contract, shortcuts, desktop types) goes in `app/desktop/src/shared`.
- **A desktop feature** goes in `app/desktop/src/main/features/`, wired up in `browser/browser.ts`.
- **A mobile feature** gets its own folder in `app/mobile/src/features/` and its routes in `app/mobile/app/`. Files like `src/ui/`, `src/state/types.ts` and `app/_layout.tsx` are shared: add to them without reorganizing them.

## Guidelines

### Cross-platform

The desktop app must run on **macOS, Linux and Windows**, and CI checks all three.

- Guard macOS-only APIs (`app.dock`, vibrancy, `systemPreferences`…) with `IS_MAC` from `app/desktop/src/main/app/platform.ts`.
- Create windows with `windowChrome()` from the same file instead of hard-coding the title bar style.
- Shortcuts use `CmdOrCtrl`, not `Cmd`.
- Build paths with `path.join`, never with `/`.
- If you can only test on one system, say which one in your pull request.

### Security

Signet runs untrusted web pages next to its own interface, so a few boundaries matter:

- **Page scripts are untrusted.** Scripts in `preload/` that run in tabs (`identity.ts`, `swipe.ts`, `passwords.ts`, `notes.ts`) never get access to `window.signet` or the interface channels. The main process re-checks everything they send (sender, tab, frame, address).
- **Interface calls go through the typed IPC contract** in `app/desktop/src/shared/ipc/contract.ts`, which validates arguments. Add new calls there instead of registering raw `ipcMain` handlers. The few channels open to page scripts are listed in `PAGE_IPC` in the same file, and each handler checks its sender itself.
- **Secrets** (passwords, link keys) are encrypted with Electron's `safeStorage`. Never write them to `state.json` or logs in clear text.

### Translations

Every visible string must exist in **both English and French**. Catalogs live in `packages/shared/src/locales/{en,fr}/`:

- `main.json`: main process (menus, dialogs, notifications)
- `ui.json`: desktop interface
- `mobile.json`: mobile app

`npm run i18n:check` checks that both languages have the same keys and the same `{{variables}}`, and that no visible text is hard-coded in the desktop interface or the mobile app.

### Code style

- TypeScript everywhere, strict mode.
- **Prettier** formats the code (no semicolons, single quotes, no trailing commas, 160 columns). Run `npm run format` before committing, or enable format on save in your editor.
- **ESLint** runs on the whole repository: `npm run lint`.
- Line endings are LF everywhere (`.gitattributes` enforces it). On Windows, keep `core.autocrlf` off for this repository.
- Keep comments short and useful: explain why, not what.
- Menus: add icons only to key items, not to every entry.

### Dependencies

- Adding a dependency needs a reason; mention it in the pull request.
- **TypeScript is pinned to 6.0.x**: typescript-eslint doesn't support newer versions yet. Don't bump it.
- On mobile, prefer libraries that work in Expo Go and install them with `npx expo install` from `app/mobile`. A native feature that Expo Go doesn't have must turn itself off cleanly there.
- Don't commit `app/mobile/ios/` or `app/mobile/android/`: they are generated by `expo prebuild`.

## Tests

| What                     | Command                                    | Where                                 |
| ------------------------ | ------------------------------------------ | ------------------------------------- |
| Unit tests (Vitest)      | `npm test`                                 | `*.test.ts` next to the code it tests |
| End-to-end (Playwright)  | `npm run test:e2e`                         | `app/desktop/tests/e2e/*.spec.ts`     |
| One e2e test             | `npm run test:e2e -- -g palette`           | Playwright options go after `--`      |
| Reuse the last e2e build | `SIGNET_E2E_SKIP_BUILD=1 npm run test:e2e` | when only the tests changed           |

Tips:

- Tab and space rules (`app/desktop/src/main/model/`) are tested without Electron using `model/testkit.ts`.
- Modules that touch Electron mock it with `vi.mock('electron', …)`.
- In end-to-end tests, wait with `expect.poll(...)`, never with fixed delays.
- Some behavior can only be checked by hand (real keyboard, real sites). It's listed in [docs/A-VERIFIER.md](docs/A-VERIFIER.md); add your case there if it applies.

[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) describes the `SIGNET_*` environment variables, which seed a sample state, take screenshots and probe the app. Use them with `--user-data-dir=<folder>` so they never touch your real profile.

## Commits

Commits follow [Conventional Commits](https://www.conventionalcommits.org/), in English, imperative mood, lowercase, no trailing period, describing the change from the user's point of view:

```
feat(tabs): make the tab sleep delay configurable
fix(mobile): open the page and tab menus without bouncing
refactor(ipc): check arguments in one typed contract
```

- **Types**: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `revert`, `security`, and `chore` only when nothing else fits.
- **Scope** (optional): the area touched, such as `tabs`, `spaces`, `passwords`, `adblock`, `mobile`, `desktop`, `linux`, `ui`.
- One logical change per commit. Don't mix a refactor and a feature in the same commit.

A check on every pull request refuses commit messages outside this format. The full rules are in [docs/git/commit-policy.md](docs/git/commit-policy.md).

## Pull requests

1. Fork the repository and create a branch from `dev`, named `<type>/<short-description>` (`fix/peek-scroll`, `feat/tab-groups`…). See [docs/git/branch-policy.md](docs/git/branch-policy.md).
2. Make your change, with tests where it makes sense.
3. Run the same checks as CI:

   ```bash
   npm run typecheck
   npm run lint
   npm run format:check
   npm run i18n:check
   npm test
   npm run test:e2e
   ```

4. Open a pull request against `dev`, titled like a commit (`feat(tabs): add tab groups`). In the description, say:
   - what changed and why (link the issue: `Closes #123`);
   - how you tested it, and on which systems;
   - for interface changes, a screenshot or a short video, in light and dark mode if relevant.

CI runs type checks, lint, formatting, translations and unit tests on macOS, Linux and Windows, end-to-end tests on macOS and Linux, and builds the mobile bundle and the Android app. A pull request needs green CI and a review before it's merged. Keep pull requests small: they get reviewed faster.

**Branches and merges.** `dev` is where work lands; `main` only receives `dev` when a release is prepared. Only maintainers merge, with a merge commit, and only they publish releases. Details: [pull requests](docs/git/pr-policy.md), [branches](docs/git/branch-policy.md), [releases](docs/git/release-policy.md).

## Reporting bugs

A good bug report includes:

- **System**: macOS, Linux (distribution) or Windows, with the version; for mobile, the phone and OS version.
- **Signet version**: the release, or the commit if you run from source (`git rev-parse --short HEAD`).
- **Steps to reproduce**, what you expected, and what happened instead.
- **Screenshots** if it's visual.
- **Logs** if a window stays blank: the interface error log is `logs/interface.log` in Signet's data folder (`~/Library/Application Support/Signet/` on macOS, `~/.config/Signet/` on Linux, `%APPDATA%\Signet\` on Windows). Check it for personal data before attaching it.
- If a **mod** may be the cause, try again after starting with `--safe-mode`.

## License

Signet is licensed under the [GNU GPL v3.0 or later](LICENSE). By contributing, you agree that your contributions are licensed under the same terms.
