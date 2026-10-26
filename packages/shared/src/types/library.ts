/** Maximum number of history entries returned per query. */
export const HISTORY_QUERY_MAX = 500

export interface HistoryEntry {
  url: string
  title: string
  favicon?: string
  visits: number
  lastVisit: number
}

export interface ArchivedEntry {
  /** Stable identifier: unique per entry (`${archivedAt}:${url}` for old ones). */
  id: string
  url: string
  title: string
  favicon?: string
  archivedAt: number
}

export type LibrarySection = 'history' | 'archive' | 'downloads' | 'spaces'

export type DownloadStatus = 'progressing' | 'paused' | 'completed' | 'cancelled' | 'interrupted'

export interface DownloadInfo {
  id: string
  filename: string
  /** Full path of the file on disk. */
  path: string
  url: string
  status: DownloadStatus
  receivedBytes: number
  /** 0 if the size is unknown. */
  totalBytes: number
  startedAt: number
  /** Launched from a private window: never written to disk. */
  private?: boolean
}
