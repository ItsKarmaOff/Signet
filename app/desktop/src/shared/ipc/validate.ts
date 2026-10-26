import type {
  BackupMode,
  BackupSection,
  BookmarkTarget,
  BrowsingDataKind,
  CaptureTarget,
  DevResponsive,
  DevToolsDock,
  DragSource,
  DropTarget,
  IconTarget,
  ImportKind,
  LibrarySection,
  LiveSource,
  PaletteMode,
  PaletteResult,
  PasswordManager,
  PermissionState,
  PipCommand,
  PopoverRequest,
  Rect,
  SpaceDraft,
  SplitLayout
} from '../types'
import { BACKUP_SECTIONS, IMPORT_KINDS } from '../types/data'

// Small IPC argument validator, dependency-free (pure TypeScript: it also runs in the sandboxed preload).
// A "guard" is a function `(value) => value is T`: the guard's type is that of the expected argument.

export type Guard<T> = ((v: unknown) => v is T) & {
  /** True for the object keys that `opt()` allows to be missing. */
  readonly optional?: true
}

type Opt<T> = Guard<T | undefined> & { readonly optional: true }

const make = <T>(fn: (v: unknown) => boolean): Guard<T> => fn as Guard<T>

/** Custom guard: we assert type `T`, the check is `fn`'s (domains already sanitized in main). */
export const custom = <T>(fn: (v: unknown) => boolean): Guard<T> => make<T>(fn)

/** Plain object (not an array, not null). */
export const isPlain = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)

/** Everything is accepted (the handler revalidates). Avoid; listed as permissive in the contract. */
export const any = <T = unknown>(): Guard<T> => make<T>(() => true)

export const str = (max?: number): Guard<string> => make<string>((v) => typeof v === 'string' && (max === undefined || v.length <= max))
export const num = (): Guard<number> => make<number>((v) => typeof v === 'number' && Number.isFinite(v))
export const int = (): Guard<number> => make<number>((v) => typeof v === 'number' && Number.isInteger(v))
export const bool = (): Guard<boolean> => make<boolean>((v) => typeof v === 'boolean')
export const nul = (): Guard<null> => make<null>((v) => v === null)

/** Union of literals: `lit('left', 'right')`. */
export const lit = <const L extends readonly (string | number | boolean)[]>(...values: L): Guard<L[number]> =>
  make<L[number]>((v) => (values as readonly unknown[]).includes(v))

/** Valeur absente (`undefined`) permise. */
export const opt = <T>(g: Guard<T>): Opt<T> =>
  Object.assign(
    make<T | undefined>((v) => v === undefined || g(v)),
    { optional: true as const }
  )

/** `null` permis. */
export const nullable = <T>(g: Guard<T>): Guard<T | null> => make<T | null>((v) => v === null || g(v))

/** One of the guards. */
export const union = <G extends Guard<unknown>[]>(...gs: G): Guard<G[number] extends Guard<infer T> ? T : never> => make((v) => gs.some((g) => g(v)))

export const arr = <T>(g: Guard<T>, max?: number): Guard<T[]> =>
  make<T[]>((v) => Array.isArray(v) && (max === undefined || v.length <= max) && v.every((x) => g(x)))

type Shape = Record<string, Guard<unknown>>
type OptKeys<S extends Shape> = { [K in keyof S]: S[K] extends { optional: true } ? K : never }[keyof S]
type ValueOf<G> = G extends Guard<infer T> ? T : never
type Infer<S extends Shape> = { [K in Exclude<keyof S, OptKeys<S>>]: ValueOf<S[K]> } & { [K in OptKeys<S>]?: Exclude<ValueOf<S[K]>, undefined> }

/** Plain object whose known keys are validated; other keys are tolerated (handlers sanitize). */
export const obj = <S extends Shape>(shape: S): Guard<Infer<S>> => make<Infer<S>>((v) => isPlain(v) && Object.entries(shape).every(([k, g]) => g(v[k])))

/** Any plain object, typed `T` (sanitized afterwards by the handler: `cleanSettings`…). */
export const plain = <T extends object>(): Guard<T> => make<T>(isPlain)

// ───────────────────────── Domains ─────────────────────────

export const rect: Guard<Rect> = obj({ x: num(), y: num(), width: num(), height: num() })

export const splitLayout: Guard<SplitLayout> = lit('columns', 'rows', 'grid')
export const paletteMode: Guard<PaletteMode> = lit('new-tab', 'edit-url', 'split')
export const devToolsDock: Guard<DevToolsDock> = lit('right', 'bottom', 'window')
export const pipCommand: Guard<PipCommand> = lit('toggle', 'back', 'forward', 'seek', 'mute', 'return', 'close')
export const permissionState: Guard<PermissionState> = lit('ask', 'allow', 'deny')
export const librarySection: Guard<LibrarySection> = lit('history', 'archive', 'downloads', 'spaces')
export const backupMode: Guard<BackupMode> = lit('add', 'replace')
export const backupSection: Guard<BackupSection> = lit(...BACKUP_SECTIONS)
export const importKind: Guard<ImportKind> = lit(...IMPORT_KINDS)
export const browsingDataKind: Guard<BrowsingDataKind> = lit('history', 'archive', 'downloads', 'cache', 'cookies', 'permissions')
export const passwordManager: Guard<PasswordManager> = lit('signet', 'bitwarden', '1password', 'proton', 'dashlane', 'lastpass', 'off')

export const dragSource: Guard<DragSource> = union(
  obj({ kind: lit('tab'), tabId: str() }),
  obj({ kind: lit('split'), id: str() }),
  obj({ kind: lit('folder'), id: str() }),
  obj({ kind: lit('group'), id: str() })
)

export const dropTarget: Guard<DropTarget> = obj({
  zone: lit('favorites', 'pinned', 'today'),
  spaceId: str(),
  folderId: opt(str()),
  groupId: opt(str()),
  index: num()
})

export const bookmarkTarget: Guard<BookmarkTarget> = obj({ folderId: opt(str()), index: num() })

export const iconTarget: Guard<IconTarget> = union(
  obj({ kind: lit('tab'), tabId: str() }),
  obj({ kind: lit('folder'), folderId: str() }),
  obj({ kind: lit('group'), groupId: str() })
)

export const captureTarget: Guard<CaptureTarget> = union(obj({ kind: lit('region'), rect }), obj({ kind: lit('visible') }), obj({ kind: lit('full') }))

/** Space draft: `cleanSpaceDraft` sanitizes everything, a plain object is enough. */
export const spaceDraft: Guard<SpaceDraft> = plain<SpaceDraft>()

export const liveSource: Guard<LiveSource> = union(obj({ kind: lit('feed'), url: str() }), obj({ kind: lit('github'), query: lit('review-requested', 'mine') }))

/** Responsive mode: plain object (main revalidates the device and sizes) or nothing. */
export const devResponsive: Guard<DevResponsive> = plain<DevResponsive>()

const renameTarget = union(
  obj({ kind: lit('tab'), tabId: str() }),
  obj({ kind: lit('folder'), folderId: str() }),
  obj({ kind: lit('group'), groupId: str() }),
  obj({ kind: lit('bookmark'), id: str() })
)

export const popoverRequest: Guard<PopoverRequest> = union(
  obj({ kind: lit('spaces') }),
  obj({ kind: lit('space-form'), spaceId: nullable(str()) }),
  obj({ kind: lit('live-folder'), folderId: nullable(str()) }),
  obj({ kind: lit('folder'), folderId: str() }),
  obj({ kind: lit('downloads') }),
  obj({ kind: lit('icon'), target: iconTarget }),
  obj({ kind: lit('rename'), target: renameTarget }),
  obj({ kind: lit('bookmarks'), folderId: nullable(str()), from: opt(num()), tabId: nullable(str()) })
)

/**
 * Palette result, as `paletteQuery` returned it: only the field that decides the action is required
 * (address, tab or action); the rest (title, icon…) is not used for execution, so it is not checked.
 */
export const paletteResult: Guard<PaletteResult> = union(
  obj({ kind: lit('url', 'search', 'suggestion', 'history', 'archived'), url: str() }),
  obj({ kind: lit('tab'), tabId: str() }),
  obj({ kind: lit('action'), action: str() })
) as unknown as Guard<PaletteResult>

// ───────────────────────── Application ─────────────────────────

/** True if `args` (list received from IPC) satisfies the guards; a missing position counts as `undefined`. Returns the faulty index, or -1. */
export function firstInvalid(guards: readonly Guard<unknown>[], args: readonly unknown[]): number {
  return guards.findIndex((g, i) => !g(args[i]))
}
