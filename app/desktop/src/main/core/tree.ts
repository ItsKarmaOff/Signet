// Tree utilities: the pure logic lives in @signet/shared; here only the default names are translated (main process `t` module).
import { newFolder as makeFolder, pruneItems as prune, type Folder } from '@signet/shared/tree'
import type { Item } from '@shared/types'
import { t } from '../app/i18n'

export * from '@signet/shared/tree'

export function newFolder(name = t('defaults.folder')): Folder {
  return makeFolder(name)
}

export function pruneItems(items: unknown, known: (tabId: string) => boolean, folders: boolean): Item[] {
  return prune(items, known, folders, t('defaults.folderFallback'))
}
