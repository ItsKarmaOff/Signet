import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import type { Plugin } from 'vite'

interface RawEmoji {
  hexcode: string
  label: string
  unicode: string
  group?: number
  order: number
  tags?: string[]
}

/** Representative emoji for each group (indexed by `group`). */
const GROUP_ICONS: Record<number, string> = { 0: '😀', 1: '👋', 3: '🐻', 4: '🍔', 5: '✈️', 6: '⚽', 7: '💡', 8: '🔣', 9: '🏁' }
const COMPONENT_GROUP = 2
/** `compact.json` does not give the version: emojis of version > 16 (empty squares on recent macOS or not). */
const TOO_RECENT = new Set([
  '1FAEB',
  '1FAEA',
  '1FAEF',
  '1FAF9',
  '1FAFA',
  '1FAC8',
  '1F9D1-200D-1FA70',
  '1FACD',
  '1FACC',
  '1FADD',
  '1F6D8',
  '1F6D9',
  '1FA8B',
  '1FA8A',
  '1FA8E',
  '1FA8C',
  '1FA8D'
])

const fold = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase()

/** A section: [key, label, icon, emojis]; an emoji: [character, label, search text (absent if equal to the label)]. */
export type EmojiSection = [string, string, string, ([string, string] | [string, string, string])[]]

/** `emojibase-data` folder, wherever npm installed it (workspaces: hoisted to the repository root). */
const dataDir = (root: string): string => dirname(createRequire(join(root, 'package.json')).resolve('emojibase-data/package.json'))

/** Picker data for one language, already filtered, sorted and grouped (only what is useful is kept). */
export function buildEmoji(locale: string, root: string): EmojiSection[] {
  const dir = join(dataDir(root), locale)
  const raw = JSON.parse(readFileSync(join(dir, 'compact.json'), 'utf8')) as RawEmoji[]
  const groups = (JSON.parse(readFileSync(join(dir, 'messages.json'), 'utf8')) as { groups: { order: number; message: string }[] }).groups
  const out: EmojiSection[] = []
  for (const g of groups) {
    if (g.order === COMPONENT_GROUP) continue
    const items = raw
      .filter((e) => e.group === g.order && !TOO_RECENT.has(e.hexcode))
      .sort((a, b) => a.order - b.order)
      .map((e): [string, string] | [string, string, string] => {
        const hay = fold([e.label, ...(e.tags ?? [])].join(' '))
        return hay === e.label ? [e.unicode, e.label] : [e.unicode, e.label, hay]
      })
    out.push([`g${g.order}`, g.message.charAt(0).toUpperCase() + g.message.slice(1), GROUP_ICONS[g.order] ?? '•', items])
  }
  return out
}

const PREFIX = 'virtual:emoji/'

/** Virtual modules `virtual:emoji/en` and `virtual:emoji/fr`: emojibase data slimmed down at build time (no skins or hexcode). */
export function emojiPlugin(root: string): Plugin {
  return {
    name: 'signet-emoji',
    resolveId: (id) => (id.startsWith(PREFIX) ? `\0${id}` : null),
    load(id) {
      if (!id.startsWith(`\0${PREFIX}`)) return null
      const locale = id.slice(PREFIX.length + 1)
      for (const f of ['compact.json', 'messages.json']) this.addWatchFile(join(dataDir(root), locale, f))
      return `export default ${JSON.stringify(buildEmoji(locale, root))}`
    }
  }
}
