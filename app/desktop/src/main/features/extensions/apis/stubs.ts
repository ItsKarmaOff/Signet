/**
 * What the safety net replaced by placeholders, per extension version (reported by the preload of each context). Kept in
 * memory for a later "Some features of this extension are not available in Signet" line in Settings.
 */
export class StubRegistry {
  private seen = new Map<string, Set<string>>()

  /** Merges a report. Returns true the first time this extension version reports (the caller logs only then). */
  record(extensionId: string, version: string, names: readonly string[]): boolean {
    const key = `${extensionId}@${version}`
    const first = !this.seen.has(key)
    const set = this.seen.get(key) ?? new Set<string>()
    for (const n of names) set.add(n)
    this.seen.set(key, set)
    return first
  }

  /** Sorted names for an extension (every version recorded so far, latest wins when `version` is given). */
  of(extensionId: string, version?: string): string[] {
    if (version) return [...(this.seen.get(`${extensionId}@${version}`) ?? [])].sort()
    const all = new Set<string>()
    for (const [key, set] of this.seen) if (key.startsWith(`${extensionId}@`)) set.forEach((n) => all.add(n))
    return [...all].sort()
  }
}
