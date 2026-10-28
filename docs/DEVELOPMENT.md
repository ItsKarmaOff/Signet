# Development

All commands run from the repository root (npm workspaces: `app/desktop`, `app/mobile`, `packages/shared`).

## Running

| Command                                          | Effect                                                                     |
| ------------------------------------------------ | -------------------------------------------------------------------------- |
| `npm run dev`                                    | Desktop in development (electron-vite, hot reload).                        |
| `npm run build` / `npm start`                    | Builds into `app/desktop/out/`, then starts that build.                    |
| `npm test`                                       | Unit tests (Vitest) for the desktop and `packages/shared`.                 |
| `npm run test:e2e`                               | End-to-end tests (Playwright + Electron), see below.                       |
| `npm run typecheck`                              | `tsc` on the desktop (main, renderer, e2e), the shared package and mobile. |
| `npm run lint`                                   | ESLint.                                                                    |
| `npm run format` / `npm run format:check`        | Prettier (writes / checks).                                                |
| `npm run i18n:check`                             | Checks that `fr` and `en` have the same keys.                              |
| `npm run icons`                                  | Regenerates the app icons.                                                 |
| `npm run dist`, `dist:linux`, `dist:win`         | macOS package, Linux AppImage, Windows installer (in `app/desktop/dist/`). |
| `npm run mobile`, `mobile:ios`, `mobile:android` | Expo (mobile app).                                                         |

Don't: run `electron-vite build` into `out/` while `npm run dev` is running. The build rewrites the files the running app reads, and the orphaned Electron can no longer be quit. This is why the e2e tests build into `out-e2e/`.

## Tests

- **Unit**: a `*.test.ts` file next to the code it tests (`src/shared`, `src/main`, `src/renderer`). The tab model (`src/main/model/`) is tested without Electron using `model/testkit.ts` (`world()`: a minimal state, the recorded effects, `shape()` to write a list on one line). Modules that touch Electron mock it with `vi.mock('electron', …)`.
- **End-to-end**: `app/desktop/tests/e2e/*.spec.ts`, a single worker, a temporary profile and one Electron instance per test.
  - `npm run test:e2e` builds the app into `app/desktop/out-e2e/`, then runs Playwright. Playwright options go after `--`: `npm run test:e2e -- -g palette`.
  - `SIGNET_E2E_SKIP_BUILD=1` reuses the last build in `out-e2e/` (useful when only the tests changed).
  - `SIGNET_E2E_MAIN=<file>` starts an entry point other than `out-e2e/main/index.js`.
  - `SIGNET_E2E=1` (set by `fixtures.ts`) exposes `globalThis.__signet`, the main process `Browser`, never in the packaged app. In a test, `signetApp.signet((b, arg) => …, arg)` runs a function in the main process with that `Browser`; `signetApp.snapshot()` returns the state sent to the interface.
  - Fixtures: `launch(profile)` (relaunches on an existing profile), `openTab`, `uiPage(app, '#overlay')` (interface view by URL fragment), `quit`, `site.url('name')` (local server whose page has the title `name`).
  - Wait for a condition with `expect.poll(...)`, not with fixed pauses.

## `SIGNET_*` environment variables

The development tools live in `app/desktop/src/main/dev/` (`seed.ts`, `capture.ts`, `probes.ts`, entry `index.ts`). `main/index.ts` imports them only if the app is not packaged; they form an `out/main/dev-*.js` chunk that `electron-builder.mjs` excludes from the package.

The sample state variables (`SIGNET_SEED` and its options) only apply with `--user-data-dir=<folder>`: never on your real profile. Arguments after `--` go to Electron. A full capture:

```
SIGNET_SEED=1 SIGNET_THEME=dark SIGNET_CAPTURE=main:/tmp/main.png npm run dev -w @signet/desktop -- -- --user-data-dir=/tmp/signet-profil
```

The capture is a PNG at an absolute path; the app then quits on its own. Targets: `main`, `settings`, `settings:<section>`, `library`.

### Sample state and appearance

| Variable                         | Effect                                                                                                                               | Example                              |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------ |
| `SIGNET_THEME`                   | Forces the `light` or `dark` theme.                                                                                                  | `SIGNET_THEME=dark`                  |
| `SIGNET_SEED`                    | Replaces the in-memory state with a sample state (2 spaces, favorites, folders, archive, routing rules). Requires `--user-data-dir`. | `SIGNET_SEED=1`                      |
| `SIGNET_ACTIVE`                  | Tab active at startup: the one whose URL contains this text.                                                                         | `SIGNET_ACTIVE=github`               |
| `SIGNET_URL`                     | Replaces the address of the active tab.                                                                                              | `SIGNET_URL=http://localhost:5173/`  |
| `SIGNET_SPLIT`, `SIGNET_SPLIT_N` | Split view of the first tabs of the day: `columns`, `rows` or `grid`, on 2 to 4 tabs.                                                | `SIGNET_SPLIT=grid SIGNET_SPLIT_N=3` |
| `SIGNET_SIDEBAR`                 | `0`: sidebar hidden.                                                                                                                 | `SIGNET_SIDEBAR=0`                   |
| `SIGNET_LAYOUT`                  | `horizontal`: tabs at the top, with two fake downloads.                                                                              | `SIGNET_LAYOUT=horizontal`           |
| `SIGNET_TABS`                    | Total number of tabs of the day (filled up with samples).                                                                            | `SIGNET_TABS=30`                     |
| `SIGNET_SPACES`                  | Total number of spaces (filled up with empty spaces).                                                                                | `SIGNET_SPACES=8`                    |
| `SIGNET_ACTIVE_SPACE`            | Index of the space active at startup.                                                                                                | `SIGNET_ACTIVE_SPACE=1`              |

### Screenshot

| Variable            | Effect                                                                                                                          | Example                                      |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| `SIGNET_CAPTURE`    | `<target>:<path.png>`: opens the window, waits, saves a capture (composited web views) and quits.                               | `SIGNET_CAPTURE=settings:privacy:/tmp/s.png` |
| `SIGNET_WAIT`       | Delay in ms before capturing the main window (3500 by default).                                                                 | `SIGNET_WAIT=8000`                           |
| `SIGNET_COMPACT`    | `1`: compact mode.                                                                                                              | `SIGNET_COMPACT=1`                           |
| `SIGNET_FLOAT`      | `toolbar`: floating toolbar shown, captured separately (`-float.png`).                                                          | `SIGNET_FLOAT=toolbar`                       |
| `SIGNET_MEDIA`      | `1`: fake media card in the sidebar.                                                                                            | `SIGNET_MEDIA=1`                             |
| `SIGNET_SITE`       | `1`: opens the site panel, captured separately (`-site.png`).                                                                   | `SIGNET_SITE=1`                              |
| `SIGNET_SPACE_FORM` | `1`: space form open.                                                                                                           | `SIGNET_SPACE_FORM=1`                        |
| `SIGNET_POPOVER`    | `spaces`, `folder`, `space-form` or `downloads`: popover of the tab row (with `SIGNET_LAYOUT=horizontal`).                      | `SIGNET_POPOVER=downloads`                   |
| `SIGNET_SWIPE`      | From -1 to 1: swipe between spaces frozen halfway.                                                                              | `SIGNET_SWIPE=0.5`                           |
| `SIGNET_DEVTOOLS`   | `right`, `bottom` or `window`: developer tools of the active tab (the detached window is captured separately, `-devtools.png`). | `SIGNET_DEVTOOLS=right`                      |

### Probes

| Variable              | Effect                                                                                                     | Example                                               |
| --------------------- | ---------------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| `SIGNET_OPEN`         | Opens the address in an active tab of the main window.                                                     | `SIGNET_OPEN=https://example.com/`                    |
| `SIGNET_OPEN_BG`      | With `SIGNET_OPEN`: opens in the background first, shows it after this delay (ms).                         | `SIGNET_OPEN_BG=3000`                                 |
| `SIGNET_EVAL`         | Script run in the active tab before the capture; result printed (`EVAL`).                                  | `SIGNET_EVAL=document.title`                          |
| `SIGNET_UIJS`         | Script (async) run in the interface before the capture; result printed (`UIJS`).                           | `SIGNET_UIJS=document.title`                          |
| `SIGNET_CONSOLE`      | `1`: copies the console warnings and errors of all pages (`CONSOLE`).                                      | `SIGNET_CONSOLE=1`                                    |
| `SIGNET_DEVELOP`      | `DevOverrides` JSON applied to the active tab (developer mode), then what the page sees of it (`DEVELOP`). | `SIGNET_DEVELOP='{"scheme":"dark","noJs":true}'`      |
| `SIGNET_DEVELOP_WAIT` | Delay in ms before that read (4500 by default).                                                            | `SIGNET_DEVELOP_WAIT=8000`                            |
| `SIGNET_INSTALL_EXT`  | Store IDs to install (comma-separated), then the state of the extensions (`EXTCHECK`).                     | `SIGNET_INSTALL_EXT=cjpalhdlnbpafiamejdnhcphjbkeiagm` |
| `SIGNET_EXT_SET`      | Settings JSON applied before the capture (order, hidden extensions).                                       | `SIGNET_EXT_SET='{"extensionOrder":[]}'`              |

### Outside `dev/`

These variables are read by code that also exists in the packaged app.

| Variable             | Effect                                                                                        | Example                        |
| -------------------- | --------------------------------------------------------------------------------------------- | ------------------------------ |
| `SIGNET_SAFE_MODE`   | `1`: starts without mods (same as `--safe-mode`), if a mod makes the interface unusable.      | `SIGNET_SAFE_MODE=1`           |
| `SIGNET_SWIPEDIAG`   | `1`: the page swipe script logs its decisions in the page console.                            | `SIGNET_SWIPEDIAG=1`           |
| `SIGNET_WEBAPPS_DIR` | Replaces the web app launcher folder (`~/Applications/Signet Apps`).                          | `SIGNET_WEBAPPS_DIR=/tmp/apps` |
| `SIGNET_AUTH_REASON` | Internal (Windows): text passed to PowerShell for the authentication window. Not set by hand. |                                |

### Tests

| Variable                | Effect                                                                                                           | Example                                    |
| ----------------------- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| `SIGNET_E2E`            | `1`: exposes `globalThis.__signet` and turns off the ad blocker (outside the packaged app). Set by the fixtures. |                                            |
| `SIGNET_E2E_MAIN`       | Entry point to start (`out-e2e/main/index.js` by default).                                                       | `SIGNET_E2E_MAIN=out/main/index.js`        |
| `SIGNET_E2E_SKIP_BUILD` | `1`: doesn't rebuild `out-e2e/` before the tests.                                                                | `SIGNET_E2E_SKIP_BUILD=1 npm run test:e2e` |

## Manual checks

The probes that needed a real keyboard or a real site (⌘Q held and its bubble, mini player on YouTube, real drag on the extension buttons) have been removed. What is left to check by hand is in [MANUAL-CHECKS.md](MANUAL-CHECKS.md); remember to add such cases there.
