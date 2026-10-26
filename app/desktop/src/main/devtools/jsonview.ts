import type { WebContents } from 'electron'
import SRC from './pages/jsonview.js?raw'
import type { Browser } from '../browser/browser'
import { currentLocale, t } from '../app/i18n'

/** Isolated world of the viewer (1001 to 1005: Boost, media, reading, translation, apps; 1007: Boost emojis). */
const WORLD = 1006
/** Beyond this, the JSON is shown as is (the browser already displays it, the tree would be too heavy). */
const MAX_BYTES = 20 * 1024 * 1024

const CSS = `
:host { color-scheme: light dark; }
.root {
  --bg: #ffffff; --bar: #f5f5f7; --line: rgb(0 0 0 / .09); --text: #1d1d1f; --text-2: #6e6e73; --text-3: #8e8e93;
  --key: #8a3ffc; --str: #1a7f37; --num: #0b62c4; --bool: #b3261e; --null: #8e8e93; --hover: rgb(0 0 0 / .04);
  --sel: rgb(10 132 255 / .12); --hit: rgb(255 204 0 / .35); --cur: rgb(255 149 0 / .55); --accent: #0a84ff;
  position: fixed; inset: 0; display: flex; flex-direction: column; background: var(--bg); color: var(--text);
  font: 13px -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
}
@media (prefers-color-scheme: dark) {
  .root {
    --bg: #1e1e20; --bar: #2a2a2d; --line: rgb(255 255 255 / .1); --text: #f2f2f7; --text-2: #aeaeb2; --text-3: #8e8e93;
    --key: #c69cff; --str: #7ee2a8; --num: #79b8ff; --bool: #ff8a80; --null: #8e8e93; --hover: rgb(255 255 255 / .05);
    --sel: rgb(10 132 255 / .25); --hit: rgb(255 214 10 / .25); --cur: rgb(255 159 10 / .5); --accent: #0a84ff;
  }
}
.bar { display: flex; align-items: center; gap: 12px; padding: 8px 12px; background: var(--bar); border-bottom: .5px solid var(--line); flex: none; }
.tabs { display: flex; gap: 2px; padding: 2px; border-radius: 8px; background: var(--hover); }
.tools { display: flex; align-items: center; gap: 6px; }
.tools.hidden, .status.hidden { display: none; }
.spacer { flex: 1; }
button {
  font: inherit; color: var(--text-2); background: transparent; border: 0; border-radius: 6px; padding: 4px 10px; cursor: default;
}
button:hover { background: var(--hover); color: var(--text); }
.tab.on { background: var(--bg); color: var(--text); box-shadow: 0 1px 2px rgb(0 0 0 / .12); }
.search {
  font: inherit; color: var(--text); background: var(--bg); border: .5px solid var(--line); border-radius: 6px;
  padding: 4px 8px; width: 200px; outline: none;
}
.search:focus { border-color: var(--accent); box-shadow: 0 0 0 3px rgb(10 132 255 / .2); }
.hits { color: var(--text-3); font-size: 12px; font-variant-numeric: tabular-nums; }
.body { flex: 1; overflow: auto; padding: 8px 0 24px; font: 12.5px/1.65 ui-monospace, 'SF Mono', Menlo, monospace; }
.body.text { padding: 0; }
.text-view { margin: 0; padding: 12px 16px; white-space: pre-wrap; word-break: break-word; font: inherit; }
.kids { padding-left: 18px; }
.row { display: flex; align-items: baseline; padding: 0 12px 0 6px; white-space: pre-wrap; word-break: break-word; }
.row:hover { background: var(--hover); }
.row.selected { background: var(--sel); }
.row.hit { background: var(--hit); }
.row.current { background: var(--cur); }
.row.close { color: var(--text-3); padding-left: 22px; }
.tw { flex: none; width: 16px; text-align: center; color: var(--text-3); font-size: 9px; }
.tw:not(.none)::before { content: '▶'; display: inline-block; transition: transform .12s ease-out; }
.tw.open::before { transform: rotate(90deg); }
.k { color: var(--key); }
.k.idx { color: var(--text-3); }
.colon, .brace { color: var(--text-3); }
.sum { color: var(--text-3); font-style: italic; }
.v.str { color: var(--str); }
.v.str a { color: inherit; text-decoration: underline; text-decoration-color: color-mix(in srgb, currentColor 40%, transparent); }
.v.number { color: var(--num); }
.v.boolean { color: var(--bool); }
.v.null { color: var(--null); }
.more { margin: 2px 0 2px 22px; color: var(--accent); }
.status {
  display: flex; align-items: center; gap: 6px; padding: 6px 12px; flex: none; min-height: 20px;
  background: var(--bar); border-top: .5px solid var(--line); color: var(--text-2); visibility: hidden;
}
.status.on { visibility: visible; }
.path { font: 12px ui-monospace, 'SF Mono', Menlo, monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
@media (prefers-reduced-motion: reduce) { .tw::before { transition: none; } }
`

/** JSON viewer: an `application/json` (or `+json`) response is shown as a collapsible tree. Settings › Development. */
export class JsonView {
  constructor(private b: Browser) {}

  get enabled(): boolean {
    return this.b.store.state.settings.jsonViewer !== false
  }

  /** After a page loads: if it is valid JSON, the tree replaces the raw display. */
  async probe(wc: WebContents): Promise<void> {
    if (!this.enabled || wc.isDestroyed() || !/^(https?|file):/i.test(wc.getURL())) return
    const locale = currentLocale()
    const labels = {
      locale,
      zeroIsOne: locale === 'fr',
      maxBytes: MAX_BYTES,
      css: CSS,
      tree: t('jsonView.tree'),
      raw: t('jsonView.raw'),
      pretty: t('jsonView.pretty'),
      expandAll: t('jsonView.expandAll'),
      collapseAll: t('jsonView.collapseAll'),
      search: t('jsonView.search'),
      noResults: t('jsonView.noResults'),
      results_one: t('jsonView.results_one'),
      results_other: t('jsonView.results_other'),
      keys_one: t('jsonView.keys_one'),
      keys_other: t('jsonView.keys_other'),
      items_one: t('jsonView.items_one'),
      items_other: t('jsonView.items_other'),
      more_one: t('jsonView.more_one'),
      more_other: t('jsonView.more_other'),
      copy: t('jsonView.copy'),
      copied: t('jsonView.copied'),
      copyPath: t('jsonView.copyPath'),
      copyValue: t('jsonView.copyValue')
    }
    try {
      await wc.executeJavaScriptInIsolatedWorld(WORLD, [{ code: `${SRC}\n;signetJsonView(${JSON.stringify(labels)})` }], true)
    } catch {
      // page left in the meantime
    }
  }
}
