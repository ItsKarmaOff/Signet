// Long-press link menu: replaces iOS's (which a WebView cannot extend). Action sheet at the bottom of the screen.
import { useRef } from 'react'
import { Platform, Share } from 'react-native'
import * as Clipboard from 'expo-clipboard'
import { findBookmarkByUrl } from '@signet/shared/mobile/bookmarks'
import { linkMenuTitle, truncateMiddle, type LinkMenuMessage } from '@signet/shared/mobile/link-menu'
import { siteOf } from '@signet/shared/site'
import { requestPrivateAccess } from '@/features/tabs'
import { haptic } from '@/lib/haptics'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ActionSheet, toast, type ActionSheetAction } from '@/ui'

interface Props {
  /** The tapped link; `null` closes the sheet. */
  link: LinkMenuMessage | null
  /** Current tab is private: tabs opened from here are private too. */
  isPrivate: boolean
  onClose: () => void
}

export function LinkMenu({ link, isPrivate, onClose }: Props) {
  const t = useT()
  // The sheet closes by sliding: keep the last link so the text does not vanish during the animation.
  const last = useRef(link)
  if (link) last.current = link
  const shown = link ?? last.current
  const href = shown?.href ?? ''
  const open = (activate: boolean) => {
    useStore.getState().openTab({ url: href, private: isPrivate, activate })
    if (!activate) {
      haptic('success')
      toast.show({ message: t('linkMenu.openedBackground') })
    }
  }

  const actions: ActionSheetAction[] = [
    { label: t('linkMenu.openNewTab'), onPress: () => open(true) },
    { label: t('linkMenu.openBackground'), onPress: () => open(false) }
  ]
  if (!isPrivate) {
    actions.push({
      label: t('linkMenu.openPrivate'),
      onPress: () => {
        void requestPrivateAccess().then((ok) => {
          if (ok) useStore.getState().openTab({ url: href, private: true })
        })
      }
    })
  }
  actions.push(
    {
      label: t('linkMenu.copy'),
      onPress: () => {
        void Clipboard.setStringAsync(href)
        haptic('success')
        toast.show({ message: t('linkMenu.copied') })
      }
    },
    { label: t('linkMenu.share'), onPress: () => void Share.share(Platform.OS === 'ios' ? { url: href } : { message: href }) },
    {
      label: t('linkMenu.addBookmark'),
      onPress: () => {
        const st = useStore.getState()
        if (findBookmarkByUrl(st.bookmarks, href)) {
          toast.show({ message: t('linkMenu.alreadyBookmarked') })
          return
        }
        st.addBookmark({ title: shown?.text || siteOf(href) || href, url: href })
        haptic('success')
        toast.show({ message: t('linkMenu.bookmarkAdded') })
      }
    }
  )

  return (
    <ActionSheet
      visible={!!link}
      onClose={onClose}
      title={shown ? linkMenuTitle(shown) : undefined}
      message={shown ? truncateMiddle(shown.href, 80) : undefined}
      cancelLabel={t('common.cancel')}
      actions={actions}
    />
  )
}
