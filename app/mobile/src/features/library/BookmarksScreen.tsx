// Bookmarks: folder tree (one screen per folder), search, edit, move, reorder, delete with undo.
import { memo, useMemo, useState } from 'react'
import { FlatList, LayoutAnimation, StyleSheet, View } from 'react-native'
import { Stack, useRouter } from 'expo-router'
import { countLinks, findBookmark, folderItems, searchBookmarks, type BookmarkFolder } from '@signet/shared/mobile/bookmarks'
import { siteOf } from '@signet/shared/site'
import type { Bookmark } from '@signet/shared/types'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ActionSheet, EmptyState, Favicon, IconButton, PressableScale, Screen, SearchField, Text, gutter, icons, space, toast, useColors } from '@/ui'
import { BookmarkEditSheet, MoveSheet, type EditTarget } from './BookmarkSheets'
import { CardRow } from './CardRow'
import { SwipeToDelete } from './SwipeToDelete'
import { exportBookmarks, importBookmarksFromFile } from './transfer'
import { openInBrowser } from './openUrl'

const FOLDER_COLOR = '#f59e0b'

export function BookmarksScreen({ folderId }: { folderId?: string }) {
  const t = useT()
  const router = useRouter()
  const tree = useStore((s) => s.bookmarks)
  const updateBookmark = useStore((s) => s.updateBookmark)
  const addBookmarkFolder = useStore((s) => s.addBookmarkFolder)
  const removeBookmark = useStore((s) => s.removeBookmark)
  const insertBookmark = useStore((s) => s.insertBookmark)
  const moveBookmark = useStore((s) => s.moveBookmark)
  const moveBookmarkBy = useStore((s) => s.moveBookmarkBy)
  const [query, setQuery] = useState('')
  const [reorder, setReorder] = useState(false)
  const [menu, setMenu] = useState(false)
  const [rowMenu, setRowMenu] = useState<Bookmark | null>(null)
  const [edit, setEdit] = useState<EditTarget | null>(null)
  const [moving, setMoving] = useState<Bookmark | null>(null)

  const folder = folderId ? findBookmark(tree, folderId)?.node : undefined
  const items = folderItems(tree, folderId)
  const searching = query.trim().length > 0
  const shown = useMemo<readonly Bookmark[]>(() => (searching ? searchBookmarks(tree, query) : (items ?? [])), [tree, items, query, searching])

  const folderTitle = folder?.kind === 'folder' ? folder.name : t('library.bookmarks')
  const gone = folderId !== undefined && items === null

  const remove = (node: Bookmark) => {
    const at = findBookmark(tree, node.id)
    if (!at) return
    LayoutAnimation.configureNext(LayoutAnimation.Presets.easeInEaseOut)
    removeBookmark(node.id)
    toast.show({
      message: node.kind === 'folder' ? t('bookmarks.folderDeleted') : t('bookmarks.deleted'),
      actionLabel: t('common.undo'),
      onAction: () => insertBookmark(node, at.parent?.id, at.index)
    })
  }

  const move = (node: Bookmark, dest: BookmarkFolder | null) => {
    const len = dest ? dest.items.length : tree.length
    LayoutAnimation.configureNext(LayoutAnimation.Presets.easeInEaseOut)
    moveBookmark(node.id, { folderId: dest?.id, index: len })
    haptic('success')
    toast.show({ message: t('bookmarks.moved', { folder: dest?.name ?? t('bookmarks.root') }) })
    setMoving(null)
  }

  const shift = (node: Bookmark, delta: number) => {
    LayoutAnimation.configureNext(LayoutAnimation.Presets.easeInEaseOut)
    haptic('selection')
    moveBookmarkBy(node.id, delta)
  }

  const onExport = () => void exportBookmarks(tree).then((message) => message && toast.show({ message }))
  const onImport = () => void importBookmarksFromFile().then((message) => message && toast.show({ message }))

  const rowActions = (node: Bookmark) => [
    { label: t('bookmarks.edit'), onPress: () => setEdit({ node }) },
    { label: t('bookmarks.move'), onPress: () => setMoving(node) },
    ...(searching ? [] : [{ label: t('bookmarks.reorder'), onPress: () => setReorder(true) }]),
    { label: t('bookmarks.delete'), destructive: true, onPress: () => remove(node) }
  ]

  if (gone) return <Screen>{null}</Screen>

  return (
    <Screen>
      <Stack.Screen
        options={{
          title: folderTitle,
          headerRight: () => (
            <View style={styles.headerActions}>
              <IconButton icon={icons.FolderPlus} label={t('bookmarks.newFolder')} tone="accent" onPress={() => setEdit({ create: { parentId: folderId } })} />
              <IconButton icon={icons.Ellipsis} label={t('bookmarks.more')} tone="accent" onPress={() => setMenu(true)} />
            </View>
          )
        }}
      />
      {tree.length === 0 ? (
        <EmptyState icon={icons.Bookmark} title={t('library.bookmarksEmpty')} message={t('bookmarks.emptyHint')} />
      ) : (
        <>
          <View style={styles.search}>
            <SearchField
              value={query}
              onChangeText={setQuery}
              clearLabel={t('common.clear')}
              placeholder={t('bookmarks.searchPlaceholder')}
              accessibilityLabel={t('bookmarks.searchPlaceholder')}
            />
          </View>
          {shown.length === 0 ? (
            searching ? (
              <EmptyState icon={icons.Search} title={t('bookmarks.noResults')} message={t('bookmarks.noResultsHint', { query: query.trim() })} />
            ) : (
              <EmptyState icon={icons.Folder} title={t('bookmarks.emptyFolder')} />
            )
          ) : (
            <FlatList
              data={shown}
              keyExtractor={(n) => n.id}
              keyboardShouldPersistTaps="handled"
              keyboardDismissMode="on-drag"
              contentContainerStyle={styles.list}
              extraData={reorder}
              renderItem={({ item, index }) => (
                <BookmarkRow
                  node={item}
                  first={index === 0}
                  last={index === shown.length - 1}
                  reorder={reorder && !searching}
                  deleteLabel={t('bookmarks.delete')}
                  moveUpLabel={t('bookmarks.moveUp')}
                  moveDownLabel={t('bookmarks.moveDown')}
                  subtitle={item.kind === 'folder' ? t('library.itemsCount', { count: countLinks(item) }) : (siteOf(item.url) ?? item.url)}
                  onOpen={(n) => (n.kind === 'folder' ? router.push(`/library/bookmarks/${n.id}`) : openInBrowser(router, n.url))}
                  onMenu={setRowMenu}
                  onDelete={remove}
                  onShift={shift}
                />
              )}
            />
          )}
        </>
      )}

      <ActionSheet
        visible={menu}
        onClose={() => setMenu(false)}
        cancelLabel={t('common.cancel')}
        actions={[
          { label: reorder ? t('bookmarks.reorderDone') : t('bookmarks.reorder'), onPress: () => setReorder((r) => !r) },
          { label: t('bookmarks.export'), onPress: onExport },
          { label: t('bookmarks.import'), onPress: onImport }
        ]}
      />
      <ActionSheet
        visible={!!rowMenu}
        onClose={() => setRowMenu(null)}
        title={rowMenu ? (rowMenu.kind === 'folder' ? rowMenu.name : rowMenu.title) : undefined}
        cancelLabel={t('common.cancel')}
        actions={rowMenu ? rowActions(rowMenu) : []}
      />
      <BookmarkEditSheet
        target={edit}
        onClose={() => setEdit(null)}
        onSaveLink={(id, patch) => updateBookmark(id, patch)}
        onSaveFolder={(target, name) => (target.node ? updateBookmark(target.node.id, { name }) : addBookmarkFolder(name, target.create?.parentId))}
      />
      <MoveSheet
        tree={tree}
        node={moving}
        currentFolderId={moving ? findBookmark(tree, moving.id)?.parent?.id : undefined}
        onClose={() => setMoving(null)}
        onPick={(dest) => moving && move(moving, dest)}
      />
    </Screen>
  )
}

interface RowProps {
  node: Bookmark
  first: boolean
  last: boolean
  reorder: boolean
  subtitle: string
  deleteLabel: string
  moveUpLabel: string
  moveDownLabel: string
  onOpen: (node: Bookmark) => void
  onMenu: (node: Bookmark) => void
  onDelete: (node: Bookmark) => void
  onShift: (node: Bookmark, delta: number) => void
}

const BookmarkRow = memo(function BookmarkRow({
  node,
  first,
  last,
  reorder,
  subtitle,
  deleteLabel,
  moveUpLabel,
  moveDownLabel,
  onOpen,
  onMenu,
  onDelete,
  onShift
}: RowProps) {
  const c = useColors()
  const isFolder = node.kind === 'folder'
  const title = isFolder ? node.name : node.title
  return (
    <CardRow first={first} last={last}>
      <SwipeToDelete label={deleteLabel} enabled={!reorder} onDelete={() => onDelete(node)}>
        <PressableScale
          accessibilityRole={isFolder ? 'button' : 'link'}
          accessibilityLabel={title}
          scaleTo={0.99}
          disabled={reorder}
          onPress={() => onOpen(node)}
          onLongPress={() => {
            haptic('medium')
            onMenu(node)
          }}
          style={[styles.row, { backgroundColor: c.surface }]}
        >
          {isFolder ? (
            <View style={[styles.folderIcon, { backgroundColor: FOLDER_COLOR }]}>
              <icons.Folder size={17} color="#ffffff" strokeWidth={2} />
            </View>
          ) : (
            <Favicon uri={node.favicon} url={node.url} size={30} />
          )}
          <View style={styles.texts}>
            <Text variant="body" numberOfLines={1}>
              {title}
            </Text>
            <Text variant="footnote" color="secondary" numberOfLines={1}>
              {subtitle}
            </Text>
          </View>
          {reorder ? (
            <View style={styles.reorder}>
              <IconButton icon={icons.ArrowUp} label={moveUpLabel} size={18} tone="accent" disabled={first} onPress={() => onShift(node, -1)} />
              <IconButton icon={icons.ArrowDown} label={moveDownLabel} size={18} tone="accent" disabled={last} onPress={() => onShift(node, 1)} />
            </View>
          ) : isFolder ? (
            <icons.ChevronRight size={18} color={c.textTertiary} strokeWidth={2} />
          ) : null}
        </PressableScale>
      </SwipeToDelete>
    </CardRow>
  )
})

const styles = StyleSheet.create({
  headerActions: { flexDirection: 'row', alignItems: 'center' },
  search: { paddingHorizontal: gutter, paddingTop: space.sm, paddingBottom: space.md },
  list: { paddingBottom: space.huge },
  row: { minHeight: 56, flexDirection: 'row', alignItems: 'center', gap: space.md, paddingHorizontal: space.lg, paddingVertical: space.sm },
  folderIcon: { width: 30, height: 30, borderRadius: 8, alignItems: 'center', justifyContent: 'center' },
  texts: { flex: 1 },
  reorder: { flexDirection: 'row', marginRight: -space.sm }
})
