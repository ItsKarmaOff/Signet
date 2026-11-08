# signet-pages

Internal pages that are documents shown inside tabs. They are web pages (rendered by CEF), not GPUI views,
and are served through the `signet://` scheme. No React, no build step: each page is HTML + vanilla JS + CSS in
`web/`, embedded with `include_str!`.

## Rust API

```rust
signet_pages::serve(url: &str, lang: &str, theme: Theme) -> Option<Response>   // Response { mime, body, status }
signet_pages::parse_document_url(url) -> Option<(DocKind, String)>             // signet://note|easel/<uuid>
signet_pages::document_csp(kind) -> String                                     // same CSP as the <meta> in the pages
signet_pages::json_viewer::{script(lang, expose), applies_to(url), is_json_content_type(ct), MAX_BYTES}
signet_pages::devices::{DEVICES, device_spec, clamp_size, ...}                 // responsive mode data
signet_pages::ROUTES                                                           // routes owned, with descriptions
```

`serve` returns `None` for an address that is not ours (including a note address whose id is not a UUID): the
caller then tries its other routes and ends on the "Address not found" page. `lang` is `fr` for any French tag,
English otherwise. `Theme::Auto` follows `prefers-color-scheme`; `Light` / `Dark` put `theme-light` /
`theme-dark` on `<html>`.

## Routes

| Address                        | Answer                                                                  |
| ------------------------------ | ----------------------------------------------------------------------- |
| `signet://note/<uuid>`         | Note page (HTML, status 200)                                            |
| `signet://easel/<uuid>`        | Easel page (HTML, status 200)                                           |
| `signet://pages/<path>`        | Static files below; unknown path: 404                                   |

The main frame of a document tab must keep the `signet://note/<id>` address (the tab shows it). The scheme must
be registered as standard and secure (so `signet://pages/...` subresources load from a `signet://` page), and
web pages must not be able to open `signet://note/...` or `signet://easel/...` (internal address guard, owned
by the app, as in Electron).

**Address decision** (shared with `signet-scripts`): the address of a document is always `signet://note/<uuid>` /
`signet://easel/<uuid>`, both displayed and loaded. Electron tabs store exactly that in `state.json` (it loaded the UI on
`#note/<id>` behind the scenes), so imported note tabs keep working with no rewrite. `signet-scripts` accepts these
addresses for the `window.signetNotes` bridge and the note IPC (`ScriptOptions::default().notes_pages` is empty).
`signet_core::urls::electron_note_url` maps a stray Electron real address (`signetui://app/index.html#note/<id>`) to
the document address.

## Files (`web/`)

- `shared/tokens.css`: design tokens (subset of the app's `tokens.css`). Light and dark through `light-dark()`.
- `shared/notes.css`: styles of both pages (port of `notes.css`).
- `shared/notes-html.js`: sanitizer (`cleanHtml`), `firstLine`, `docIcon` (tab favicon), `setTabLook`.
  Port of `notesHtml.ts`. Exposed as `window.SignetNotesHtml` (and `module.exports` for node tests).
- `shared/common.js`: reads the injected configuration, `t(key)`, icons, DOM helpers. `window.Signet`.
- `shared/spring.js`: spring and rubber band helpers used by the Easel (port of `lib/spring.ts`).
- `note/index.html`, `note/note.js`: Note page (port of `Note.tsx`).
- `easel/index.html`, `easel/easel.js`: Easel page (port of `Easel.tsx`).
- `json/jsonview.js`, `json/jsonview.css`: JSON viewer (port of `jsonview.js` and its CSS), injected, not served.

## Bridge expected from signet-scripts (`preload/notes.ts`)

The page script must define, in the main frame of a `signet://note/<id>` or `signet://easel/<id>` tab (the id is
taken from the address on the Rust side, never from an argument):

```ts
window.signetNotes = {
  load(): Promise<NotesEnv>        // { kind, id, doc: NoteDoc | EaselDoc | null, hue }
  save(patch): Promise<boolean>    // Note: { title, html }; Easel: { title, items, view }; false = refused (too large)
  remove(): Promise<void>          // deletes the document and closes its tabs
}
```

`NotesEnv`, `NoteDoc`, `EaselDoc` and `EaselItem` keep the shapes of `app/desktop/src/shared/notes.ts`. The pages
read nothing else. `window.signet` (getLocale, onAppearance...) is not used: language and theme are injected by
`serve`. The script may be installed late: the pages call `window.signetNotes?.load()` once at startup, so it
must exist when the document starts running scripts (inject it at document start). Other needs from the app:

- `window.open(url, '_blank')` (Cmd/Ctrl click on a link in a note) must open a new tab.
- The title (`document.title`) and the favicon (`<link rel="icon">`, SVG data URL) are the tab's title and icon.
- Appearance changes at runtime: call `Signet.setTheme('light' | 'dark' | 'auto')` in the page, or reload.

## Translations

Texts come from `packages/shared/src/locales/{en,fr}/ui.json` (sections `notes`, `easel`, plus `common.cancel`),
read at build time with `include_str!` and injected as a flat map (`"notes.bold": "Bold"`) in
`<script type="application/json" id="signet-config">` together with `lang` and `theme`. A missing French key falls
back to English. The JSON viewer labels are the `jsonView` section of `main.json`.

## Theming

`tokens.css` defines colors with `light-dark()` and `color-scheme: light dark`. Forcing is done by the class on
`<html>`. The space hue is the CSS variable `--h`, set by the page from `NotesEnv.hue`.

## Security

Each page has a `Content-Security-Policy` (meta, also available through `document_csp`): no inline script, scripts
only from `signet://pages`. Note and Easel text goes through `cleanHtml` on display and on paste (text tags,
http(s) and mailto links, http(s) or raster data images, `ul.checklist`, `li[data-checked]`). The sanitizer runs
in the page only: saving is validated by the Rust side of the notes feature (signet-features), as in Electron.

## JSON viewer

Not a served page: the app evaluates `json_viewer::script(lang, false)` in a tab when a load finishes, if
`json_viewer::applies_to(url)` (`http(s):` or `file:`) and `Settings.jsonViewer != false`. The script checks
`document.contentType` itself (see `is_json_content_type`), the 20 MB limit and `JSON.parse`, and returns `ok`,
`already`, `not-json`, `too-big` or `invalid`. The UI sits in a closed shadow root styled with a constructed
sheet (works under a strict page CSP). CEF has no isolated world, so it runs in the page world and leaves only a
non-enumerable `window.__signetJson` marker (`expose = true` keeps the shadow root there, for tests).

## Responsive design mode

Everything in `Responsive.tsx` is interface chrome (bar, handles in the margin), drawn by GPUI. Only the device
list is here, as data (`devices.rs`): ids, sizes, density, mobile flag, user agent kind. The emulation itself
belongs to signet-web.

## Not in this crate

Reader mode is a shadow-root overlay injected in the page (`features/reader.ts`), so it belongs with the page
scripts. `signet://urls`, `signet://version`, the not-found page and the error pages are generated by
signet-features. The welcome and quit screens are interface views.

## Tests

```sh
CARGO_TARGET_DIR=target-pages cargo test -p signet-pages
node --test crates/signet-pages/tests      # JS tests (sanitizer rules, icon, spring helpers)
```
