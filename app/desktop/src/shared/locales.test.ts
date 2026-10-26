import { describe, expect, it } from 'vitest'
import { FIXED_SHORTCUTS, SHORTCUTS, reservedShortcuts } from './shortcuts'
import enMain from '@signet/shared/locales/en/main.json'
import enUi from '@signet/shared/locales/en/ui.json'
import frMain from '@signet/shared/locales/fr/main.json'
import frUi from '@signet/shared/locales/fr/ui.json'

// Catalogs in packages/shared/src/locales. Complements scripts/i18n-check.mjs: same keys and same variables, plus what the script does not see
// (context/plural keys, <1>… tags, shortcut labels, well-formed JSON).
const catalogs: Record<string, Record<string, Json>> = {
  en: { main: enMain as Json, ui: enUi as Json },
  fr: { main: frMain as Json, ui: frUi as Json }
}

type Json = { [k: string]: Json | string }
const load = (lng: string, ns: string): Json => catalogs[lng][ns]

function flatten(obj: Json, prefix = '', out: Record<string, string> = {}): Record<string, string> {
  for (const [k, v] of Object.entries(obj)) {
    const key = prefix ? `${prefix}.${k}` : k
    if (typeof v === 'object') flatten(v, key, out)
    else out[key] = v
  }
  return out
}

const vars = (s: string) =>
  [...s.matchAll(/\{\{\s*([\w.]+)\s*(?:,[^}]*)?\}\}/g)]
    .map((m) => m[1])
    .sort()
    .join(',')
// <Trans> component tags (<1>…</1>): must also match exactly.
const tags = (s: string) =>
  [...s.matchAll(/<\/?(\d+)\s*\/?>/g)]
    .map((m) => m[0])
    .sort()
    .join(',')

describe.each(['main', 'ui'])('catalogue %s', (ns) => {
  const en = flatten(load('en', ns))
  const fr = flatten(load('fr', ns))

  it('has the same keys in English and French', () => {
    expect(Object.keys(fr).sort()).toEqual(Object.keys(en).sort())
  })

  it('keeps the same {{variables}} per key', () => {
    const diff = Object.keys(en)
      .filter((k) => k in fr && vars(en[k]) !== vars(fr[k]))
      .map((k) => `${k}: en[${vars(en[k])}] fr[${vars(fr[k])}]`)
    expect(diff).toEqual([])
  })

  it('keeps the same <n> tags per key', () => {
    const diff = Object.keys(en)
      .filter((k) => k in fr && tags(en[k]) !== tags(fr[k]))
      .map((k) => k)
    expect(diff).toEqual([])
  })

  it('n’a aucune valeur vide', () => {
    for (const [k, v] of [...Object.entries(en), ...Object.entries(fr)]) expect(v.trim(), k).not.toBe('')
  })

  it('has balanced braces', () => {
    for (const [k, v] of [...Object.entries(en), ...Object.entries(fr)]) {
      expect((v.match(/\{\{/g) ?? []).length, k).toBe((v.match(/\}\}/g) ?? []).length)
    }
  })

  it('has consistent sets of plural and context suffixes', () => {
    // If `x_one` exists, `x_other` must exist (in each language).
    for (const cat of [en, fr]) {
      for (const k of Object.keys(cat)) {
        if (k.endsWith('_one')) expect(cat[k.replace(/_one$/, '_other')], k).toBeDefined()
      }
    }
  })
})

describe('shortcut labels', () => {
  const ui = flatten(load('en', 'ui'))
  const uiFr = flatten(load('fr', 'ui'))
  const labels = new Set<string>([
    ...SHORTCUTS.map((s) => s.label),
    ...FIXED_SHORTCUTS.map((s) => s.label),
    ...reservedShortcuts(true, false).map((r) => r.label),
    ...reservedShortcuts(false, true).map((r) => r.label)
  ])

  it.each([...labels])('%s existe en en et fr', (label) => {
    expect(ui[label], 'en').toBeTypeOf('string')
    expect(uiFr[label], 'fr').toBeTypeOf('string')
  })
})
