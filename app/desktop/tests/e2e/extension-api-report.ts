// Turns the probe reports (tests/extensions/api-probe) into a readable Markdown table.
import { EXTENSION_API_CATALOG, type NamespaceSpec } from '../../src/shared/extension-api-catalog'

export const CONTEXTS = ['worker', 'popup', 'options', 'iframe'] as const
export type ContextName = (typeof CONTEXTS)[number]

interface MemberReport {
  kind: 'method' | 'event' | 'constant' | 'class'
  chrome: string | null
  browser: string | null
  chromeEvent?: string
  browserEvent?: string
}
interface NamespaceReport {
  unsupported: boolean
  contentScriptOnly: boolean
  chrome: string | null
  browser: string | null
  same: boolean
  members: Record<string, MemberReport>
}
interface CallReport {
  status: 'resolved' | 'rejected' | 'threw' | 'timeout' | 'missing' | 'skipped'
  value?: string
  message?: string
}
export interface ProbeReport {
  context: string
  hasChrome?: boolean
  hasBrowser?: boolean
  browserIsChrome?: boolean | null
  chromeFrozen?: boolean | null
  chromeKeys?: string[]
  browserKeys?: string[]
  extensionId?: string | null
  href?: string | null
  errors: { kind: string; message: string }[]
  namespaces?: Record<string, NamespaceReport>
  calls?: Record<string, CallReport>
}
export type Reports = Partial<Record<ContextName, ProbeReport | null>>

const ABBR: Record<string, string> = { function: 'fn', object: 'obj', number: 'num', string: 'str', boolean: 'bool', undefined: 'undef' }
const abbr = (t: string): string => ABBR[t] ?? t
const esc = (s: string): string => s.replace(/\|/g, '\\|').replace(/\s+/g, ' ')
const clip = (s: string, n: number): string => (s.length > n ? s.slice(0, n) + '...' : s)

/** `fn`, `ev`, `ev!` (event whose addListener/hasListener/hasListeners fails), `-` (missing), `(c)` / `(b)` when only chrome / only browser has it. */
function memberCell(m: MemberReport | undefined): string {
  if (!m) return '?'
  const label = (type: string, ev?: string): string =>
    m.kind === 'event' && type === 'object' ? (ev === 'ok' ? 'ev' : ev?.startsWith('skipped') ? 'ev?' : 'ev!') : abbr(type)
  if (m.chrome && m.browser) return label(m.chrome, m.chromeEvent) + (m.chrome !== m.browser ? ` (b: ${abbr(m.browser)})` : '')
  if (m.chrome) return `${label(m.chrome, m.chromeEvent)} (c)`
  if (m.browser) return `${label(m.browser, m.browserEvent)} (b)`
  return '-'
}

function namespaceCell(n: NamespaceReport | undefined): string {
  if (!n) return '?'
  if (n.chrome && n.browser) return n.same ? 'obj (=)' : 'obj (c+b)'
  if (n.chrome) return 'obj (c)'
  if (n.browser) return 'obj (b)'
  return '-'
}

function callText(c: CallReport | undefined): string {
  if (!c) return '?'
  if (c.status === 'resolved') return c.value && c.value !== 'undefined' ? `resolved ${c.value}` : 'resolved'
  if (c.status === 'timeout') return 'timeout'
  return `${c.status}${c.message ? `: ${c.message}` : ''}`
}

/** Results of one call across contexts: one text if identical everywhere, otherwise `worker: x; popup: y`. */
function callSummary(label: string, reports: Reports): string {
  const per = CONTEXTS.map((c) => [c, callText(reports[c]?.calls?.[label])] as const).filter(([, t]) => t !== '?')
  if (!per.length) return ''
  if (new Set(per.map(([, t]) => t)).size === 1) return clip(per[0][1], 90)
  return per.map(([c, t]) => `${c}: ${clip(t, 90)}`).join('; ')
}

function* specs(catalog: Record<string, NamespaceSpec>, prefix = ''): Generator<[string, NamespaceSpec]> {
  for (const [name, spec] of Object.entries(catalog)) {
    yield [prefix + name, spec]
    if (spec.children) yield* specs(spec.children, `${prefix}${name}.`)
  }
}

export function buildMarkdown(reports: Reports, extras: { extensionId: string; logs: string[]; electron?: string; platform: string }): string {
  const out: string[] = []
  out.push('# Extension API probe', '')
  out.push(`Extension \`${extras.extensionId}\`, ${extras.platform}${extras.electron ? `, Electron ${extras.electron}` : ''}.`, '')
  out.push(
    'Cell legend: `fn` function, `ev` event whose addListener/hasListener/hasListeners work, `ev!` event object with a failing method, `ev?` event not exercised (it crashes Signet), `obj` object, `num`/`str`/`bool` constants, `-` missing, `?` context not reported.',
    'A suffix `(c)` / `(b)` means only `chrome` / only `browser` has it; `(=)` means `browser[ns] === chrome[ns]`.',
    ''
  )

  out.push('## Contexts', '', '| context | chrome | browser | browser === chrome | chrome frozen | errors | url |', '|---|---|---|---|---|---|---|')
  for (const c of CONTEXTS) {
    const r = reports[c]
    if (!r) out.push(`| ${c} | not reported | | | | | |`)
    else
      out.push(
        `| ${c} | ${r.hasChrome ? 'yes' : 'no'} | ${r.hasBrowser ? 'yes' : 'no'} | ${r.browserIsChrome === null ? 'n/a' : r.browserIsChrome ? 'yes' : 'NO'} | ${r.chromeFrozen ? 'yes' : 'no'} | ${r.errors.length} | ${esc(r.href ?? '')} |`
      )
  }
  out.push('')
  const errs = CONTEXTS.flatMap((c) => (reports[c]?.errors ?? []).map((e) => `- ${c}: ${e.kind}: ${esc(e.message)}`))
  if (errs.length) out.push('### Errors', '', ...errs, '')

  const catalogNames = new Set([...specs(EXTENSION_API_CATALOG)].map(([p]) => p.split('.')[0]))
  const outside = (c: ContextName, key: 'chromeKeys' | 'browserKeys') => (reports[c]?.[key] ?? []).filter((k) => !catalogNames.has(k)).join(', ') || '(none)'
  out.push('### Keys of `chrome` / `browser` outside the catalog', '')
  for (const c of CONTEXTS) if (reports[c]) out.push(`- ${c} chrome: ${outside(c, 'chromeKeys')}; browser: ${outside(c, 'browserKeys')}`)
  out.push('')

  out.push('## Harmless calls', '', `| call | ${CONTEXTS.join(' | ')} |`, `|---|${CONTEXTS.map(() => '---').join('|')}|`)
  const labels = [...new Set(CONTEXTS.flatMap((c) => Object.keys(reports[c]?.calls ?? {})))]
  for (const l of labels) out.push(`| \`${l}\` | ${CONTEXTS.map((c) => clip(esc(callText(reports[c]?.calls?.[l])), 70)).join(' | ')} |`)
  out.push('')

  out.push(
    '## Namespaces and members',
    '',
    `| namespace | member | ${CONTEXTS.join(' | ')} | call result |`,
    `|---|---|${CONTEXTS.map(() => '---').join('|')}|---|`
  )
  for (const [path, spec] of specs(EXTENSION_API_CATALOG)) {
    const flags = `${spec.unsupported ? ' (unsupported)' : ''}${spec.contentScriptOnly ? ' (content script only)' : ''}`
    out.push(`| **${path}**${flags} | | ${CONTEXTS.map((c) => namespaceCell(reports[c]?.namespaces?.[path])).join(' | ')} | |`)
    const names = [...spec.methods, ...spec.events, ...Object.keys(spec.constants ?? {}), ...(spec.classes ?? [])]
    for (const name of names) {
      const cells = CONTEXTS.map((c) => memberCell(reports[c]?.namespaces?.[path]?.members[name]))
      out.push(`| ${path} | ${name} | ${cells.join(' | ')} | ${esc(callSummary(`${path}.${name}`, reports))} |`)
    }
  }
  out.push('')

  if (extras.logs.length) out.push('## Main process log lines', '', ...extras.logs.map((l) => `- \`${esc(l)}\``), '')
  return out.join('\n')
}
