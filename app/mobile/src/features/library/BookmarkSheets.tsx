// Bookmark sheets: edit a link, rename / create a folder, choose a destination folder.
import { useEffect, useState } from 'react'
import { StyleSheet, View } from 'react-native'
import { moveDestinations, parseBookmarkUrl, type BookmarkFolder } from '@signet/shared/mobile/bookmarks'
import type { Bookmark } from '@signet/shared/types'
import { useT } from '@/lib/i18n'
import { ListRow, ListSection, ModalSheet, TextField, icons, space } from '@/ui'

export interface EditTarget {
  /** Absent for a new folder. */
  node?: Bookmark
  /** Create a folder in `parentId` (root if absent). */
  create?: { parentId?: string }
}

/** Edit a link (title + validated address), rename a folder or create one. */
export function BookmarkEditSheet({
  target,
  onClose,
  onSaveLink,
  onSaveFolder
}: {
  target: EditTarget | null
  onClose: () => void
  onSaveLink: (id: string, patch: { title: string; url: string }) => void
  onSaveFolder: (target: EditTarget, name: string) => void
}) {
  const t = useT()
  const node = target?.node
  const isFolder = target ? !node || node.kind === 'folder' : false
  const [title, setTitle] = useState('')
  const [url, setUrl] = useState('')
  const [touched, setTouched] = useState(false)

  useEffect(() => {
    if (!target) return
    setTouched(false)
    if (!node) {
      setTitle(t('bookmarks.folderDefaultName'))
      setUrl('')
    } else if (node.kind === 'folder') {
      setTitle(node.name)
      setUrl('')
    } else {
      setTitle(node.title)
      setUrl(node.url)
    }
    // Only opening on a new target resets the fields.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target])

  const parsed = isFolder ? null : parseBookmarkUrl(url)
  const urlError = !isFolder && touched && !parsed ? t('bookmarks.urlInvalid') : undefined
  const valid = isFolder ? title.trim().length > 0 : !!parsed
  const save = () => {
    if (!target) return
    if (!valid) {
      setTouched(true)
      return
    }
    if (isFolder) onSaveFolder(target, title.trim())
    else if (node && parsed) onSaveLink(node.id, { title: title.trim() || parsed, url: parsed })
    onClose()
  }

  const heading = !node ? t('bookmarks.newFolder') : isFolder ? t('bookmarks.renameFolder') : t('bookmarks.editBookmark')
  return (
    <ModalSheet
      visible={!!target}
      title={heading}
      cancelLabel={t('common.cancel')}
      saveLabel={!node ? t('bookmarks.add') : t('bookmarks.save')}
      saveDisabled={!valid && touched}
      onSave={save}
      onClose={onClose}
    >
      <TextField
        label={isFolder ? t('bookmarks.folderName') : t('bookmarks.titleLabel')}
        value={title}
        onChangeText={setTitle}
        autoFocus={isFolder}
        returnKeyType={isFolder ? 'done' : 'next'}
        onSubmitEditing={isFolder ? save : undefined}
        selectTextOnFocus={isFolder}
        clearButtonMode="while-editing"
      />
      {isFolder ? null : (
        <TextField
          label={t('bookmarks.urlLabel')}
          value={url}
          onChangeText={setUrl}
          onBlur={() => setTouched(true)}
          error={urlError}
          autoCapitalize="none"
          autoCorrect={false}
          keyboardType="url"
          returnKeyType="done"
          onSubmitEditing={save}
        />
      )}
    </ModalSheet>
  )
}

/** Choose the folder to move a bookmark to (the root, then the folders with their depth). */
export function MoveSheet({
  tree,
  node,
  currentFolderId,
  onClose,
  onPick
}: {
  tree: readonly Bookmark[]
  node: Bookmark | null
  currentFolderId?: string
  onClose: () => void
  onPick: (folder: BookmarkFolder | null) => void
}) {
  const t = useT()
  const destinations = node ? moveDestinations(tree, node.id) : []
  return (
    <ModalSheet visible={!!node} title={t('bookmarks.moveTitle')} cancelLabel={t('common.cancel')} onClose={onClose}>
      <ListSection>
        <ListRow
          title={t('bookmarks.root')}
          icon={icons.Bookmark}
          iconColor="#f59e0b"
          selected={currentFolderId === undefined}
          disabled={currentFolderId === undefined}
          onPress={() => onPick(null)}
        />
        {destinations.map(({ folder, depth }) => (
          <ListRow
            key={folder.id}
            title={folder.name}
            left={
              <View style={[styles.indent, { marginLeft: depth * space.lg }]}>
                <icons.Folder size={22} color="#f59e0b" strokeWidth={1.8} />
              </View>
            }
            selected={folder.id === currentFolderId}
            disabled={folder.id === currentFolderId}
            onPress={() => onPick(folder)}
          />
        ))}
      </ListSection>
    </ModalSheet>
  )
}

const styles = StyleSheet.create({
  indent: { width: 29, alignItems: 'center' }
})
