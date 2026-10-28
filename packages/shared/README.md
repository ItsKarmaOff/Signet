# @signet/shared

**Platform-free** TypeScript code, shared by the desktop app (Electron) and the mobile app (React Native / Expo). It is a "source" package: no build step, the apps import the `.ts` files directly.

## What's inside

| Module             | Content                                                                                                                                                                                                     |
| ------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `types/`           | The data model: tabs, spaces, bookmarks, settings, history, passwords, developer tools.                                                                                                                     |
| `model.ts`         | Model factories and sanitizers (`newSpace`, `newTab`, `cleanSpaceDraft`).                                                                                                                                   |
| `tree.ts`          | Tree utilities: tab lists, folders, split views, groups.                                                                                                                                                    |
| `tab-list.ts`      | Reducer for a flat tab list with an active tab (used by mobile).                                                                                                                                            |
| `split.ts`         | Split view: computing the rectangles and dividers, inserting a tab.                                                                                                                                         |
| `urls.ts`          | Input to address or search (`toUrl`, `searchUrl`).                                                                                                                                                          |
| `site.ts`          | `siteOf`: the domain of a site (public suffix list).                                                                                                                                                        |
| `routes.ts`        | Rules that route links to a space.                                                                                                                                                                          |
| `feeds.ts`         | Reading RSS and Atom feeds (dynamic folders).                                                                                                                                                               |
| `ids.ts`           | `newId`: a unique identifier, with a fallback if `crypto.randomUUID` is missing.                                                                                                                            |
| `i18n.ts`          | Creating the i18next instance (`createI18n`), picking the language (`resolveLocale`).                                                                                                                       |
| `mobile/`          | Mobile app logic: `workspace` (spaces and tabs), `history`, `bookmarks`, `settings`, `search`, `suggestions`, `color` (space tints), `persist` (saved document, versions, migrations). One test per module. |
| `locales/{en,fr}/` | The text catalogs: `main.json` (desktop, main process), `ui.json` (desktop, interface), `mobile.json`.                                                                                                      |

## The rule

- **Only code that runs everywhere goes here**: in the Electron main process, in the web interface, and in Hermes (the JavaScript engine of React Native).
- **Never import** `electron`, `node:*`, the DOM (`document`, `window`…), or `react-native`. The package's `tsconfig` has neither the DOM types nor the Node types (`"types": []`, `lib: ES2023`): a forbidden import makes `npm run typecheck` fail.
- **Depends on no app**: never import `app/desktop` or `app/mobile`.
- **Anything platform-specific is injected** by the caller: translated default names, identifiers… (see `newFolder` in `tree.ts` and `cleanSpaceDraft` in `model.ts`).
- **The only non-ECMAScript global** the package needs is `URL`. `src/globals.d.ts` declares it (it is a types file, it changes nothing at runtime). In consumers, the DOM or Node types already provide `URL`: so the desktop excludes this file from its `tsconfig.web.json`.

## How to use it

Import by subpath, never from a general index:

```ts
import { siteOf } from '@signet/shared/site'
import { toUrl } from '@signet/shared/urls'
import type { TabInfo } from '@signet/shared/types'
import { openTab } from '@signet/shared/mobile/workspace'
import en from '@signet/shared/locales/en/ui.json'
```

The mappings are in `package.json` (`exports`): `./types` points to `src/types/index.ts`, `./locales/*` to `src/locales/*`, `./*` to `src/*.ts`.

## Adding a module

1. Create `src/my-module.ts`. It can be imported right away as `@signet/shared/my-module`.
2. Write **pure functions**: same inputs, same outputs, nothing that touches the disk, the network or the screen.
3. Write its test next to it: `src/my-module.test.ts` (Vitest).
4. Run `npm test -w @signet/shared`, then `npm run typecheck` from the root.
5. Add a row to the table above.

For visible text, add the key in **both** languages, then run `npm run i18n:check`.

## Not to be confused with

`app/desktop/src/shared` (alias `@shared/*`) is **another** "shared" folder: it holds the code shared by the processes of the **desktop only** (IPC contract, shortcuts, test fixtures, window and extension types). Mobile does not see it. To choose: if mobile may need it, it goes here; otherwise, it goes there.

The desktop re-exports this package's types from `@shared/types`, so that its code keeps importing all its types from the same place.

## Tests

Tests sit next to the code (`*.test.ts`). To run them: `npm test -w @signet/shared`. The code map is in [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md).
