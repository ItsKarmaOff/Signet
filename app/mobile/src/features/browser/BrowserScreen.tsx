// Main screen: the page full screen, the glass bar at the bottom (address field and buttons) that collapses on scroll,
// and on top, on demand: page menu, find in page, reader mode.
import { useCallback, useEffect, useRef, useState, type ComponentRef } from 'react'
import { BackHandler, Keyboard, Linking, Platform, Share, StyleSheet, View } from 'react-native'
import * as Clipboard from 'expo-clipboard'
import { useFocusEffect, useLocalSearchParams, useRouter } from 'expo-router'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { useSharedValue, withSpring } from 'react-native-reanimated'
import { useShallow } from 'zustand/react/shallow'
import { findClearScript, findScript, findStepScript, type FindResult } from '@signet/shared/mobile/find'
import { READER_EXTRACT_SCRIPT, type ReaderArticle } from '@signet/shared/mobile/reader'
import { isPrivateTab, tabLocation, visibleTabIds } from '@signet/shared/mobile/workspace'
import { haptic } from '@/lib/haptics'
import { askActiveComputer, sendOpenToComputer, useActiveComputers } from '@/features/link'
import { captureThumbnail } from '@/features/tabs'
import { useT } from '@/lib/i18n'
import { registerLiveCapture } from '@/lib/liveCapture'
import { useCurrentTab, useIsBookmarked, useStore } from '@/state'
import { springs, toast, useColors } from '@/ui'
import { BottomBar, collapsedBarHeight } from './BottomBar'
import { BrowserView, type BrowserViewHandle } from './BrowserView'
import { FIND_BAR_HEIGHT, FindBar } from './FindBar'
import type { LinkMenuMessage } from '@signet/shared/mobile/link-menu'
import { LinkMenu } from './LinkMenu'
import { PageMenu, type PageMenuActions } from './PageMenu'
import { ReaderOverlay, type ReaderState } from './ReaderOverlay'
import { StartView } from './StartView'
import { SpaceSwipe } from './spaceSwipe'
import { MAX_ZOOM, MIN_ZOOM, ZOOM_STEP, patchTabView, useTabView, useViewStore } from './viewStore'

/** Number of WebViews kept alive (the displayed tab and the most recent ones): going back to one does not reload the page. */
const MAX_LIVE = 3
/** Continuous scroll (px) in the same direction before collapsing or expanding the toolbar. */
const SCROLL_HYSTERESIS = 48
/** Duration (ms) after a bar change during which it is not toggled again. */
const TOGGLE_SETTLE_MS = 350

export function BrowserScreen() {
  const t = useT()
  const c = useColors()
  const router = useRouter()
  const insets = useSafeAreaInsets()
  const tab = useCurrentTab()
  const tabId = tab?.id
  const url = tab?.url ?? ''
  const tabCount = useStore((s) => visibleTabIds(s.workspace).length)
  const isPrivate = useStore((s) => !!tab && isPrivateTab(s.workspace, tab.id))
  const zone = useStore((s) => {
    const loc = tab ? tabLocation(s.workspace, tab.id) : null
    return loc ? (loc.kind === 'private' ? 'private' : loc.zone) : 'today'
  })
  const bookmarked = useIsBookmarked(url)
  const view = useTabView(tabId)
  const computers = useActiveComputers()

  // Live WebViews: the displayed tab first, then the most recently used. A tab without an address shows the start screen, with no WebView.
  const [recent, setRecent] = useState<string[]>([])
  const hasUrl = !!url
  useEffect(() => {
    if (tabId && hasUrl) setRecent((prev) => [tabId, ...prev.filter((id) => id !== tabId)].slice(0, MAX_LIVE))
  }, [tabId, hasUrl])
  const wanted = tabId && hasUrl ? [tabId, ...recent.filter((id) => id !== tabId)].slice(0, MAX_LIVE) : recent
  const liveTabs = useStore(
    useShallow((s) =>
      wanted
        .map((id) => s.workspace.tabs[id])
        .filter((x) => !!x && !!x.url)
        .sort((a, b) => (a.id < b.id ? -1 : 1))
    )
  )
  const privateIds = useStore(useShallow((s) => s.workspace.priv.tabIds))
  const tabIds = useStore(useShallow((s) => Object.keys(s.workspace.tabs)))
  useEffect(() => useViewStore.getState().prune(tabIds), [tabIds])

  const webBox = useRef<ComponentRef<typeof View>>(null)
  const handles = useRef<Record<string, BrowserViewHandle | null>>({})
  const current = () => (tabId ? handles.current[tabId] : null) ?? null

  // The other features (command bar, spaces) snapshot the displayed tab before leaving it.
  useEffect(() => {
    if (!tabId) return
    return registerLiveCapture(() => captureThumbnail(tabId, webBox))
  }, [tabId])

  // Toolbar: 0 = full, 1 = collapsed.
  const collapsed = useSharedValue(0)
  const collapsedTarget = useRef(0)
  // Extreme point since the last state change (the highest if the bar is full, the lowest if it is collapsed):
  // the bar only toggles after SCROLL_HYSTERESIS px in the other direction. Small back-and-forth jitter in the position (Android,
  // pages that adjust their scroll) no longer makes it bounce.
  const anchorY = useRef(0)
  const lastToggle = useRef(0)
  const setCollapsed = useCallback(
    (value: 0 | 1) => {
      if (collapsedTarget.current === value) return
      collapsedTarget.current = value
      lastToggle.current = Date.now()
      collapsed.value = withSpring(value, springs.snappy)
    },
    [collapsed]
  )
  const onScroll = useCallback(
    (y: number, max: number) => {
      if (y <= 0) {
        anchorY.current = 0
        setCollapsed(0)
        return
      }
      if (y > max + 1) return // bounce at the bottom of the page
      const folded = collapsedTarget.current === 1
      // During the bar animation, only the extreme point is tracked.
      const settling = Date.now() - lastToggle.current < TOGGLE_SETTLE_MS
      if (folded ? y > anchorY.current : y < anchorY.current) anchorY.current = y
      if (settling) return
      if (!folded && y > 80 && y - anchorY.current > SCROLL_HYSTERESIS) {
        anchorY.current = y
        setCollapsed(1)
      } else if (folded && anchorY.current - y > SCROLL_HYSTERESIS) {
        anchorY.current = y
        setCollapsed(0)
      }
    },
    [setCollapsed]
  )

  // Find, reader, menu: specific to the displayed tab.
  const [findOpen, setFindOpen] = useState(false)
  const [findResult, setFindResult] = useState<FindResult | null>(null)
  const [menuOpen, setMenuOpen] = useState(false)
  const [reader, setReader] = useState<ReaderState | null>(null)
  const [linkMenu, setLinkMenu] = useState<LinkMenuMessage | null>(null)
  const onLinkMenu = useCallback((link: LinkMenuMessage) => {
    haptic('medium')
    setLinkMenu(link)
  }, [])
  const [keyboard, setKeyboard] = useState(0)
  useEffect(() => {
    if (!findOpen) return
    const show = Keyboard.addListener(Platform.OS === 'ios' ? 'keyboardWillShow' : 'keyboardDidShow', (e) =>
      // Android too: edge to edge, the window no longer shrinks under the keyboard.
      setKeyboard(e.endCoordinates.height)
    )
    const hide = Keyboard.addListener(Platform.OS === 'ios' ? 'keyboardWillHide' : 'keyboardDidHide', () => setKeyboard(0))
    return () => {
      show.remove()
      hide.remove()
      setKeyboard(0)
    }
  }, [findOpen])

  const closeFind = () => {
    setFindOpen(false)
    setFindResult(null)
    current()?.inject(findClearScript())
    Keyboard.dismiss()
  }

  // Switching tabs closes what was open on the previous one and shows the bar again.
  const previousTab = useRef(tabId)
  useEffect(() => {
    if (previousTab.current === tabId) return
    handles.current[previousTab.current ?? '']?.inject(findClearScript())
    previousTab.current = tabId
    setFindOpen(false)
    setFindResult(null)
    setMenuOpen(false)
    setReader(null)
    setLinkMenu(null)
    setCollapsed(0)
  }, [tabId, setCollapsed])
  // Empty or new page: the bar is full again.
  useEffect(() => {
    setCollapsed(0)
    anchorY.current = 0
  }, [hasUrl, setCollapsed])

  // Android back button: closes whatever is open, then goes back in the page.
  const canGoBack = !!tab?.canGoBack
  useFocusEffect(
    useCallback(() => {
      if (Platform.OS !== 'android') return
      const sub = BackHandler.addEventListener('hardwareBackPress', () => {
        if (reader) setReader(null)
        else if (menuOpen) setMenuOpen(false)
        else if (findOpen) {
          setFindOpen(false)
          setFindResult(null)
          Keyboard.dismiss()
        } else if (canGoBack && tabId) handles.current[tabId]?.goBack()
        else return false
        return true
      })
      return () => sub.remove()
    }, [reader, menuOpen, findOpen, canGoBack, tabId])
  )

  const bottomPad = collapsedBarHeight(insets.bottom)
  const webBottom = findOpen ? Math.max(bottomPad, keyboard + FIND_BAR_HEIGHT) : bottomPad

  const share = () => {
    if (url) void Share.share(Platform.OS === 'ios' ? { url } : { message: url })
  }
  const copyLink = () => {
    if (!url) return
    void Clipboard.setStringAsync(url)
    haptic('success')
    toast.show({ message: t('pageMenu.copied') })
  }

  // "Open on computer": directly if there is only one, otherwise ask which.
  const sendToComputer = async () => {
    if (!url) return
    const linkId = computers.length === 1 ? computers[0]!.link.linkId : await askActiveComputer(t('link.openOnComputerTitle'))
    if (!linkId) return
    const name = computers.find((x) => x.link.linkId === linkId)?.link.desktop.name ?? ''
    const sent = await sendOpenToComputer(linkId, url)
    haptic(sent ? 'success' : 'warning')
    toast.show({ message: sent ? t('link.sentTo', { name }) : t('link.sendFailed', { name }) })
  }

  const openReader = () => {
    setMenuOpen(false)
    setReader({ status: 'loading' })
    current()?.inject(READER_EXTRACT_SCRIPT)
    // No answer (page blocking scripts, WebView in error): say so rather than spin forever.
    setTimeout(() => setReader((r) => (r?.status === 'loading' ? { status: 'empty' } : r)), 8000)
  }
  const onReaderArticle = useCallback((article: ReaderArticle | null) => {
    setReader((r) => (r ? (article ? { status: 'ready', article } : { status: 'empty' }) : r))
  }, [])

  const actions: PageMenuActions = {
    toggleBookmark: () => {
      if (!tab) return
      setMenuOpen(false)
      const added = useStore.getState().toggleBookmark({ title: tab.customTitle || tab.title || url, url, favicon: tab.favicon })
      haptic('success')
      toast.show({ message: added ? t('pageMenu.bookmarkAdded') : t('pageMenu.bookmarkRemoved') })
    },
    find: () => {
      setMenuOpen(false)
      setFindOpen(true)
    },
    reader: openReader,
    fillPassword: () => {
      setMenuOpen(false)
      current()?.fillPassword()
    },
    zoom: (dir) => {
      if (!tabId) return
      haptic('selection')
      patchTabView(tabId, { zoom: Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, view.zoom + dir * ZOOM_STEP)) })
    },
    setDesktop: (on) => {
      if (!tabId) return
      patchTabView(tabId, { desktop: on })
      setMenuOpen(false)
    },
    copyLink: () => {
      setMenuOpen(false)
      copyLink()
    },
    share: () => {
      setMenuOpen(false)
      share()
    },
    openInBrowser: () => {
      setMenuOpen(false)
      if (url) void Linking.openURL(url)
    },
    openOnComputer: () => {
      setMenuOpen(false)
      void sendToComputer()
    },
    togglePin: () => {
      if (!tabId) return
      setMenuOpen(false)
      useStore.getState().pinTab(tabId, zone !== 'pinned')
      haptic('light')
    },
    toggleFavorite: () => {
      if (!tabId) return
      setMenuOpen(false)
      useStore.getState().favoriteTab(tabId, zone !== 'favorites')
      haptic('light')
    },
    closeTab: () => {
      if (!tabId) return
      setMenuOpen(false)
      useStore.getState().closeTab(tabId)
      toast.show({ message: t('tabs.closed'), actionLabel: t('common.undo'), onAction: () => useStore.getState().undoCloseTab() })
    }
  }

  // DEV: `/?dev=menu|menupw|find|reader|linkmenu|pwsave|pwchip` opens this panel (QA screenshots cannot tap the screen).
  const { dev } = useLocalSearchParams<{ dev?: string }>()
  useEffect(() => {
    if (!__DEV__ || !dev || !tabId || (dev === 'reader' && tab?.loading)) return
    const id = setTimeout(() => {
      if (dev === 'menu' || dev === 'menupw') setMenuOpen(true)
      else if (dev === 'find') setFindOpen(true)
      else if (dev === 'reader') openReader()
      else if (dev === 'linkmenu') setLinkMenu({ type: 'linkMenu', href: 'https://en.wikipedia.org/wiki/Felidae', text: 'Felidae', x: 180, y: 520 })
    }, 1500)
    return () => clearTimeout(id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dev, tabId, tab?.loading])

  if (!tab) return <View style={[styles.screen, { backgroundColor: c.background }]} />

  const loading = tab.loading && view.progress < 1

  return (
    <View style={[styles.screen, { backgroundColor: c.background, paddingTop: url ? insets.top : 0 }]}>
      <View ref={webBox} collapsable={false} style={styles.web}>
        {liveTabs.map((tb) => (
          <BrowserView
            key={tb.id}
            ref={(h) => {
              handles.current[tb.id] = h
            }}
            tab={tb}
            visible={tb.id === tab.id}
            isPrivate={privateIds.includes(tb.id)}
            bottomInset={tb.id === tab.id ? webBottom : bottomPad}
            onScroll={onScroll}
            onFind={setFindResult}
            onReader={onReaderArticle}
            onLinkMenu={onLinkMenu}
          />
        ))}
        {!url ? (
          <StartView topInset={insets.top} bottomInset={bottomPad} onOpen={(target) => useStore.getState().updateTab(tab.id, { url: target, loading: true })} />
        ) : null}
        {reader ? (
          <ReaderOverlay
            state={reader}
            topInset={insets.top}
            bleed={url ? insets.top : 0}
            bottomInset={webBottom}
            onClose={() => setReader(null)}
            onOpenLink={(target) => {
              setReader(null)
              useStore.getState().updateTab(tab.id, { url: target, loading: true })
            }}
          />
        ) : null}
      </View>

      {findOpen ? (
        <FindBar
          result={findResult}
          safeBottom={insets.bottom}
          onQuery={(q) => current()?.inject(q ? findScript(q) : findClearScript())}
          onStep={(d) => {
            haptic('selection')
            current()?.inject(findStepScript(d))
          }}
          onClose={closeFind}
        />
      ) : (
        <View style={styles.bar} pointerEvents="box-none">
          <SpaceSwipe>
            <BottomBar
              tab={tab}
              isPrivate={isPrivate}
              progress={view.progress}
              tabCount={tabCount}
              collapsed={collapsed}
              safeBottom={insets.bottom}
              onAddress={() => {
                if (collapsedTarget.current === 1) setCollapsed(0)
                else router.push('/command')
              }}
              onCopyAddress={copyLink}
              onBack={() => current()?.goBack()}
              onForward={() => current()?.goForward()}
              onReloadStop={() => (loading ? current()?.stopLoading() : current()?.reload())}
              onShare={share}
              onTabs={() => {
                void captureThumbnail(tab.id, webBox).finally(() => router.push('/tabs'))
              }}
              onMenu={() => {
                haptic('light')
                setMenuOpen(true)
              }}
            />
          </SpaceSwipe>
        </View>
      )}

      <LinkMenu link={linkMenu} isPrivate={isPrivate} onClose={() => setLinkMenu(null)} />

      {menuOpen ? (
        <PageMenu
          tab={tab}
          zone={zone}
          bookmarked={bookmarked}
          desktop={view.desktop}
          zoom={view.zoom}
          canFill={view.canFill}
          canSendToComputer={computers.length > 0}
          actions={actions}
          onClose={() => setMenuOpen(false)}
        />
      ) : null}
    </View>
  )
}

const styles = StyleSheet.create({
  screen: { flex: 1 },
  web: { flex: 1 },
  bar: { position: 'absolute', left: 0, right: 0, bottom: 0 }
})
