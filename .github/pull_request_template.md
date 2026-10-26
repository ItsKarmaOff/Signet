<!-- Title: <type>(<scope>): <short description>, e.g. fix(tabs): keep the scroll position when peeking. Target: dev. See docs/git/pr-policy.md -->

## What and why

<!-- What this pull request changes, and why. Link the issue it fixes. -->

Closes #

## How I tested it

<!-- Steps, and the systems you tried it on. -->

- [ ] macOS
- [ ] Linux
- [ ] Windows
- [ ] iOS
- [ ] Android

## Screenshots

<!-- For interface changes: before and after, in light and dark mode if relevant. Delete this section otherwise. -->

## Checklist

- [ ] `npm run typecheck`, `npm run lint`, `npm run format:check`, `npm run i18n:check` and `npm test` pass
- [ ] `npm run test:e2e` passes, or the change doesn't touch the desktop app
- [ ] New visible text exists in English and French
- [ ] macOS-only APIs are guarded (`IS_MAC`, `windowChrome()`)
- [ ] Tests added or updated where it makes sense
- [ ] Manual checks added to `docs/A-VERIFIER.md` if the change can't be tested automatically
