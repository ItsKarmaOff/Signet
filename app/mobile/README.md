# Signet mobile

The Signet mobile app (iOS and Android), in React Native with Expo (SDK 58) and `expo-router`. It runs in **Expo Go** for development, and can also be installed as a compiled app on an iPhone (see below).

The **foundations** are in place: navigation, design system, store with persistence, translations. Each feature (browser, command bar, tabs, spaces, library, settings, onboarding) has its own folder and routes; several are still placeholder screens (`StubScreen`).

## Layout

```
app/mobile/
  app/                 ROUTES (expo-router). Thin files: they re-export a screen from src/features/.
    _layout.tsx        root: gestures, safe areas, theme, storage loading, main stack, toasts
    index.tsx          /                      browser
    tabs.tsx           /tabs                  tab switcher (full-screen modal)
    command.tsx        /command               command bar (transparent modal)
    onboarding.tsx     /onboarding            first launch (the only route while onboardingDone is false)
    library/           /library, /library/history, /library/bookmarks   (modal, own stack)
    settings/          /settings, /settings/search-engine|homepage|data|about   (modal, own stack)
    spaces/edit/[id]   /spaces/edit/<id|new>  space editor (sheet)
  src/
    features/<feature>/    the screens and components of a feature: browser, command, tabs, spaces, library, settings, onboarding
                           (each folder has an index.ts that says what is built there)
    ui/                    the design system (theme, text, components): `import { Button } from '@/ui'`
    state/                 the store (zustand), its slices, its persistence: `import { useStore } from '@/state'`
    lib/                   utilities with no screen: i18n (`useT`), haptics
```

The `@/` alias points to `src/` (tsconfig `paths`, supported by Metro). Imports between features go through their `index.ts`.

**Who touches what.** Each feature lives in its folder under `src/features/` and in its routes under `app/`. To avoid conflicts between people working in parallel: `src/ui/`, `src/state/types.ts`, `app/_layout.tsx` and the `mobile.json` catalogs are shared files, you add to them without reorganizing (one store slice = one file in `src/state/slices/`).

## What comes from the shared package

The logic that is not specific to the phone lives in `@signet/shared` and has its Vitest tests. What stays here is the display and whatever touches the platform (WebView, storage, haptics, device language).

| Import                                  | Contents                                                                                                                   |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `@signet/shared/mobile/workspace`       | Spaces and tabs: `Workspace`, `openTab`, `closeTab`, `moveTab`, `pinTab`, `addSpace`, private browsing... (pure functions) |
| `@signet/shared/mobile/history`         | History: `addVisit`, `searchHistory` (frecency), `groupByDay`, `topSites`                                                  |
| `@signet/shared/mobile/bookmarks`       | Bookmarks (tree of links and folders): add, move, search                                                                   |
| `@signet/shared/mobile/settings`        | `MobileSettings` type, default values, `sanitizeSettings`                                                                  |
| `@signet/shared/mobile/search`          | Search engines, `resolveInput(input, engine)`                                                                              |
| `@signet/shared/mobile/suggestions`     | `suggest()`: command bar suggestions                                                                                       |
| `@signet/shared/mobile/color`           | Space colors (accent, tinted backgrounds, gradient), computation ported from desktop                                       |
| `@signet/shared/mobile/persist`         | Saved document: version, migrations, tolerant reading                                                                      |
| `@signet/shared/{types,urls,site,i18n}` | Common types (`TabInfo`, `Space`, `Bookmark`...), addresses, a site's domain, `createI18n`                                 |
| `@signet/shared/locales/*/mobile.json`  | The app's strings, in English and French                                                                                   |

## The store

A single zustand store (`useStore`), split into slices that are thin wrappers around the pure functions of the shared package. Read: `useStore((s) => s.settings.theme)` or a hook from `src/state/hooks.ts` (`useCurrentTab`, `useVisibleTabs`, `useActiveSpace`, `useSpaces`, `usePrivateMode`, `useSettings`, `useIsBookmarked(url)`). Write: call an action, `useStore.getState().openTab({ url })` or `const openTab = useStore((s) => s.openTab)`.

| Slice       | State                           | Main actions                                                                                                                                                                                                                                             |
| ----------- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `workspace` | `workspace`, `lastClosed`       | `openTab`, `closeTab`, `undoCloseTab`, `activateTab`, `updateTab`, `moveTab`, `pinTab`, `favoriteTab`, `closeTodayTabs`, `setPrivateMode`, `closePrivateTabs`, `setActiveSpace`, `addSpace`, `editSpace`, `removeSpace`, `moveSpace`, `reportNavigation` |
| `history`   | `history`                       | `addVisit`, `removeHistoryEntry`, `clearHistory(since?)`                                                                                                                                                                                                 |
| `bookmarks` | `bookmarks`                     | `addBookmark`, `addBookmarkFolder`, `removeBookmark`, `removeBookmarkByUrl`, `updateBookmark`, `moveBookmark`, `toggleBookmark`                                                                                                                          |
| `settings`  | `settings`                      | `setSettings(patch)`, `resetSettings`                                                                                                                                                                                                                    |
| `ui`        | `ui.onboardingDone`, `hydrated` | `setOnboardingDone`                                                                                                                                                                                                                                      |

Rules to know:

- **Private tabs** live in `workspace.priv`, never saved, never in history. `reportNavigation(tabId, page, patch)` is the entry point for visits: it updates the tab and only adds to history outside private browsing and once the page has loaded.
- **Persistence**: `state/persistence.ts` reads AsyncStorage at launch (`hydrated` becomes true, before that the root only shows the background), then writes 600 ms after the last change, and right away when the app goes to the background. A single versioned JSON document (`signet.mobile.state`); an unreadable read gives the default values, never an error. The old skeleton format (`signet.tabs.v1`) is migrated once.
- **Language**: `settings.language` is applied by `persistence.ts`. In a component, `const t = useT()`.

## The design system (`src/ui/`)

Everything is imported from `@/ui`. The theme (`useTheme()`, `useColors()`) follows **the hue of the active space** (same computation as desktop, in OKLCH) and the light / dark / auto setting; private browsing switches to a dark violet-gray.

| Component                               | Role and main props                                                                                                                                                                           |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Text`                                  | `variant` (largeTitle, title1-3, headline, body, callout, subhead, footnote, caption, caption2), `color` (text, secondary, tertiary, accent, danger, onAccent, or a color), `weight`, `align` |
| `GlassBar`                              | Glass bar (iOS blur, tinted veil; opaque on Android or with "Reduce Transparency"): `edge` (top, bottom, none), `floating`, `intensity`                                                       |
| `IconButton`                            | `icon`, `label` (required), `onPress`, `onLongPress`, `size`, `tone`, `filled`, `badge`, `disabled`; 44 pt hit area, light haptic                                                             |
| `Button`                                | `title`, `onPress`, `variant` (primary, secondary, plain, destructive), `size` (md, lg), `icon`, `loading`, `fullWidth`                                                                       |
| `ListSection` + `ListRow` + `SwitchRow` | iOS Settings-style lists. Row: `title`, `subtitle`, `icon`, `iconColor`, `left`, `value`, `right`, `selected`, `chevron`, `destructive`, `onPress`                                            |
| `Screen`, `Sheet`                       | `Screen`: background, safe areas (`edges`), `scroll`, `padded`. `Sheet`: the content of a sheet (`title`, `doneLabel`, `onDone`)                                                              |
| `TextField`, `SearchField`              | Fields: `label`, `error`, `right`; `SearchField`: `value`, `onChangeText`, `clearLabel`. Fixed height: **never `flex: 1` on a field in a column**                                             |
| `Favicon`                               | `uri`, `url`, `size`; falls back to a letter on a tinted background                                                                                                                           |
| `EmptyState`                            | `icon`, `title`, `message`, `action`                                                                                                                                                          |
| `toast` + `ToastHost`                   | `toast.show({ message, actionLabel, onAction, duration })` from anywhere; the host is mounted in `app/_layout.tsx`                                                                            |
| `SegmentedControl`                      | `options` (`value`, `label`, `icon`), `value`, `onChange`                                                                                                                                     |
| `PressableScale`                        | Base of everything that can be pressed: spring, 0.96 scale, reduced motion respected; `haptic`                                                                                                |
| `icons`                                 | The lucide icons in use (`icons.Search`). To add one, one line in `src/ui/icons.ts` (one import per icon: the whole package is too heavy)                                                     |
| `useStackOptions`, `HeaderDone`         | Stack headers in the theme colors; the Done button of a modal                                                                                                                                 |
| `StubScreen`                            | Placeholder screen for routes not built yet                                                                                                                                                   |

Tokens: `space`, `gutter`, `radius`, `type`, `shadow`, `springs` (`default`, `snappy`, `gentle`, `press`). Motion goes through Reanimated; `useReduceMotion()` and `useReduceTransparency()` give the accessibility settings.

## Recipes

**Add a screen.** 1) Write the screen in `src/features/<feature>/MyScreen.tsx` (start with `<Screen>`), export it from the folder's `index.ts`. 2) Create the route: `app/<path>.tsx` with `export { MyScreen as default } from '@/features/<feature>'`. 3) If it needs a title or a presentation (modal, sheet), declare it in the folder's `_layout.tsx` (`<Stack.Screen name="..." options={{ title: t('...') }} />`). 4) Navigate with `router.push('/path')`. All strings go through `t()`.

**Add a setting.** 1) `packages/shared/src/mobile/settings.ts`: the field in `MobileSettings`, its default value, its sanitizing in `sanitizeSettings`. 2) A test in `settings.test.ts`. 3) A row in `src/features/settings/SettingsScreen.tsx` (`SwitchRow`, `ListRow`...) that calls `setSettings({ field })`. 4) The strings in both `mobile.json` files. No change to the store or to persistence: settings are sanitized on read, an old save gets the default value.

**Add a store action.** 1) The pure logic (a function that takes the state and returns the new state) in `packages/shared/src/mobile/<module>.ts`, with its test. 2) The action's type in `src/state/types.ts`, in the relevant slice. 3) Its implementation (one `set((s) => ({ ... }))` line) in `src/state/slices/<slice>.ts`. New data to save: add it to `LoadedState` and `PersistedState` in `persist.ts`, sanitize it in `parsePersisted`, and if the format breaks the old one, bump `PERSIST_VERSION` and add a step in `migrate`.

**Add an icon, a component.** Icon: `src/ui/icons.ts`. Shared component: `src/ui/MyComponent.tsx` and a line in `src/ui/index.ts`. A component specific to one feature stays in `src/features/<feature>/`.

## Development: reaching a state through a link

The simulator cannot tap the screen from the command line. In development (`__DEV__`), the `app/dev/[action].tsx` route sets up the app state and goes back to the home screen; it does nothing in production. You open it with a deep link:

```bash
xcrun simctl openurl booted "exp://127.0.0.1:8081/--/dev/seed"
xcrun simctl io booted screenshot capture.png
```

| Action                                                  | Effect                                                                                                           |
| ------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `dev/seed`                                              | 3 spaces, tabs (favorites, pinned, today's), history over several days, bookmarks with a folder; onboarding done |
| `dev/reset`                                             | fresh state: onboarding comes back                                                                               |
| `dev/lang-fr`, `dev/lang-en`                            | language                                                                                                         |
| `dev/theme-dark`, `dev/theme-light`, `dev/theme-system` | theme                                                                                                            |
| `dev/private-on`, `dev/private-off`                     | private browsing                                                                                                 |
| `dev/open?url=https://…`                                | opens a tab on that address                                                                                      |
| `dev/space?i=1`                                         | activates the 2nd space                                                                                          |
| `dev/toast`                                             | shows a toast                                                                                                    |
| `dev/download?url=…`, `dev/download-private?url=…`      | downloads a file (iOS; the private tab does not add it to the list)                                              |
| `dev/pw-seed`                                           | 3 demo credentials in the Keychain (the id of the first one is written to the log)                               |
| `dev/blocking-on`, `dev/blocking-off`                   | content blocking                                                                                                 |
| `dev/clear-site-data`                                   | clears site data (native); the log gives the number of sites before and after                                    |

Other useful links: `/?dev=menu|menupw|find|reader|linkmenu|pwsave|pwchip` (page menu, with or without "Fill password"; long-press menu on a link; "Save password for <site>?" card; key button), `/command?newTab=1&q=swift` (prefilled input, to see the network suggestions), `/?dev=menu|find|reader` (opens the page menu, find in page or reader on the current tab), `/onboarding?devPage=2`, `/tabs`, `/command?newTab=1`, `/settings/data`, `/spaces/edit/new`. After changing the persisted state, restart Expo Go to start again from a clean navigation stack.

**In the simulator, without Expo Go** (content blocking, Keychain, icons: native modules). `npx expo prebuild --platform ios` (back up `package.json` first, prebuild rewrites its scripts), then `xcodebuild -workspace ios/Signet.xcworkspace -scheme Signet -configuration Debug -destination id=<udid> -derivedDataPath ios/build-sim build` and `xcrun simctl install` / `launch dev.signet.mobile`. The development app loads the JavaScript from Metro (`npx expo start`, port 8081). The simulator asks "Open in Signet?" for every `signet://` link: to drive the app without tapping the screen, run `python3 scripts/qa-server.py &`, the app (in `__DEV__`) reads one command per second from `localhost:8099` and navigates (`curl -G --data-urlencode 'p=/dev/seed' localhost:8099/set`). Face ID: `xcrun simctl spawn <udid> notifyutil -s com.apple.BiometricKit.enrollmentChanged 1 && notifyutil -p com.apple.BiometricKit.enrollmentChanged` to enroll a face, `notifyutil -p com.apple.BiometricKit_Sim.pearl.match` to match it.

## Installing the compiled app on an iPhone

A real app on the Home Screen, which works without the Mac, with its own page in Settings › Apps and the icon picker. With a free Apple Account, it expires after 7 days: run the command again.

1. Once only: an Apple Account in Xcode › Settings › Accounts, and Developer Mode on the iPhone (Settings › Privacy & Security).
2. iPhone plugged in and unlocked, from the repo root: `npm run mobile:device`. The script generates the native project (`ios/`, ignored by git), builds in Release and installs the app (first run: 15 to 25 minutes).
3. On first launch: Settings › General › VPN & Device Management › your Apple Account › Trust.

The signing team is `ios.appleTeamId` in `app.json`. For another iPhone: `IOS_DEVICE=<udid> npm run mobile:device` (`xcrun devicectl list devices` gives the UDID).

## Trying the app on an iPhone with Expo Go

Expo Go is a free app that runs your code without you having to build or sign anything.

1. On the iPhone, install **Expo Go** from the App Store. It must support **SDK 58** (update it if needed). If your Expo Go is too old or too new, the app will refuse to open and say so.
2. On your computer, install the dependencies (once only): `npm install`, at the repo root.
3. Still at the root, run: `npm run mobile`. Expo starts and shows a **QR code** in the terminal.
4. On the iPhone, open the **Camera** app and point it at the QR code. Tap the "Open in Expo Go" banner. The app loads.
5. Edit a file in `app/mobile` or `packages/shared/src`: the app reloads by itself.

The iPhone and the computer must be on **the same Wi-Fi network**. If that is not possible (company network, Wi-Fi that isolates devices), use a tunnel:

```bash
cd app/mobile
npx expo start --tunnel
```

## iOS simulator (Mac only)

1. Install Xcode from the App Store.
2. Open Xcode, then **Xcode › Settings › Components**: download an **iOS runtime** (the simulator has none by default).
3. At the repo root: `npm run mobile:ios`. Expo opens the simulator and launches the app in it.

## Android

**Try it.** On an Android phone, install **Expo Go** from the Play Store, then run `npm run mobile` and scan the QR code from Expo Go (it has its own scanner). Or, with an Android Studio emulator already running: `npm run mobile:android`.

**Build the app (alternate icons, default browser, biometric lock).** None of this exists in Expo Go.

1. Install Android Studio (Android SDK, bundled JDK 17) and plug in a phone with USB debugging, or start an emulator.
2. From `app/mobile`: `npx expo prebuild --platform android` generates `android/` (ignored by git; do not touch `ios/`). Prebuild rewrites the scripts in `package.json`: back it up first and restore the copy afterwards (a `git checkout` would lose dependencies that were added and not committed yet).
3. `npx expo run:android --variant release` builds and installs, or open `android/` in Android Studio. For a distributable `.apk` / `.aab`: EAS (`npx eas-cli build --platform android --profile preview`, an `eas.json` still has to be created) or `cd android && ./gradlew assembleRelease` (you then need a signing key).

**Signing key for published versions.** Android only installs an update if it is signed with the same key as the version already installed. So all published versions are signed with a single key, kept outside the repo. Losing it means never being able to update the app again (it would have to be uninstalled, data included).

1. Create the key, once only (OpenSSL is enough, no need for Java). Pick a strong password, asked twice:

   ```bash
   openssl req -x509 -newkey rsa:4096 -sha256 -days 10000 -nodes -subj "/CN=Signet" -keyout signet-key.pem -out signet-cert.pem
   openssl pkcs12 -export -name signet -inkey signet-key.pem -in signet-cert.pem -out signet-release.keystore
   rm signet-key.pem signet-cert.pem
   ```

2. Save `signet-release.keystore` and its password in a password manager, plus a copy off the computer. Never commit them.
3. Give them to the Build workflow, as repository secrets:

   ```bash
   base64 -i signet-release.keystore | gh secret set ANDROID_KEYSTORE_BASE64
   gh secret set ANDROID_KEYSTORE_PASSWORD   # paste the password
   ```

The workflow then signs the APK with this key (alias `signet`) and prints in its log the SHA-256 fingerprint of the certificate, which must stay the same from one version to the next. A release (tag `v…`) fails right away if the secrets are missing. A manual run without them signs with the Expo debug key, for testing only.

**What is configured in `app.json` (`android`).**

| Topic           | Setting                                                                                                                                                                                                                                     |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Icon            | Adaptive icon: background, logo and monochrome silhouette (Android 13+ themed icons) in `assets/android/<id>-background\|foreground\|monochrome.png`, generated by `npm run icons -w @signet/mobile` from the desktop SVGs.                 |
| Alternate icons | The 7 icons (`Ocean`... `Or`) become `activity-alias` entries in the manifest. On Android, changing the icon can close the app: the picker screen asks for confirmation, and `useSyncAppIcon` reads the real icon again on the next launch. |
| Permissions     | Camera, microphone, precise and approximate location (never in the background), biometrics, vibration, network. Removed: external storage, media, overlay windows. The file picker of sites goes through the system picker.                 |
| Default browser | `VIEW` + `BROWSABLE` + `DEFAULT` intent filter for `http` and `https` (Settings › Apps › Default apps › Browser app). `app/+native-intent.tsx` sends these links to `app/open.tsx`, which opens a tab.                                      |
| Keyboard, back  | `softwareKeyboardLayoutMode: resize`; predictive back gesture disabled (the back button goes back in the page through `BackHandler`).                                                                                                       |
| Languages       | `expo-localization` declares `en` and `fr` (Android 13+ per-app language picker). The app name is the same in both.                                                                                                                         |

**Not tested (no Android device or SDK on the development Mac).** Only `expo export --platform android` (the bundle builds) and `expo prebuild --platform android` (the manifest, icon resources and aliases are generated) have been checked. Still to try on a real phone: icon rendering (safe zone, themed icon), icon switching and launcher behavior, choosing Signet as the default browser and opening an external link, camera, microphone and location prompts in sites, file uploads, downloads, the keyboard (`resize` with edge-to-edge mode), the back button, the biometric lock, screenshots of private tabs (not blocked yet, see `FLAG_SECURE`).

## Downloads

In-app downloads (Library › Downloads, a folder visible in the Files app) exist **on iOS only**; on Android, downloads go through the system download manager. A download started from a **private tab** is neither listed nor saved: the file goes to the app cache (purged by the system) and the toast offers to open it or save it to Files.

## Useful commands

From the repo root:

| Command                  | Effect                                                           |
| ------------------------ | ---------------------------------------------------------------- |
| `npm run mobile`         | Starts Expo (QR code).                                           |
| `npm run mobile:ios`     | Starts Expo and opens the iOS simulator.                         |
| `npm run mobile:android` | Starts Expo and opens Android.                                   |
| `npm run typecheck`      | Checks types, mobile included.                                   |
| `npm run lint`           | ESLint (mobile included: `app/mobile/app` and `app/mobile/src`). |
| `npm run i18n:check`     | Checks translations and the absence of hardcoded text (mobile).  |
| `npm test`               | Vitest tests (the mobile logic is tested in the shared package). |

## Known limits

- **Expo Go first.** Prefer libraries included in Expo Go (`npx expo install` from `app/mobile`). A native feature outside Expo Go (like the app icons) must disable itself cleanly in Expo Go. No `.apk` or release yet.
- **iOS uses WebKit.** Apple forces its web engine on all iOS browsers: desktop features that depend on Chromium (extensions, engine-level ad blocker...) will not come over as they are.
- **A single copy of React.** `metro.config.js` forces the bundler to use the mobile app's `react` (19.3.0, the one Expo expects), not the desktop one hoisted to the root by npm. Consequence: `npx expo-doctor` reports a duplicate `react`. **This is expected**, not an error.
- **The blur** of `GlassBar` only exists on iOS; on Android, a nearly opaque veil replaces it.
