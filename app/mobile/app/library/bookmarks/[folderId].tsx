import { useLocalSearchParams } from 'expo-router'
import { BookmarksScreen } from '@/features/library'

export default function BookmarkFolderRoute() {
  const { folderId } = useLocalSearchParams<{ folderId: string }>()
  return <BookmarksScreen folderId={folderId} />
}
