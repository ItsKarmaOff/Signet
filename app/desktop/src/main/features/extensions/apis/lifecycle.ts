/** A store that keeps data per extension across runs. */
export interface PersistedStore {
  /** Ids that have anything stored. */
  knownIds(): string[]
  /** The extension is uninstalled: drop what is stored for it. */
  forget(id: string): void
}

/**
 * Data kept for an extension until it is uninstalled. An unload (reload of an unpacked extension, store update) must not
 * touch it: only `forget` (uninstall) and `collect` (startup, for extensions that vanished) do.
 */
export class PersistedData {
  constructor(private stores: () => PersistedStore[]) {}

  forget(id: string): void {
    for (const s of this.stores()) s.forget(id)
  }

  /** Forgets every id with stored data that is not installed; returns them. */
  collect(installed: ReadonlySet<string>): string[] {
    const known = new Set(this.stores().flatMap((s) => s.knownIds()))
    const gone = [...known].filter((id) => !installed.has(id))
    for (const id of gone) this.forget(id)
    return gone
  }
}
