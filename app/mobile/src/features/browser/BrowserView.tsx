import { forwardRef, type ComponentRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react'
import { Linking, Platform, StyleSheet, View, type NativeScrollEvent, type NativeSyntheticEvent } from 'react-native'
import { WebView, type WebViewMessageEvent, type WebViewNavigation } from 'react-native-webview'
import type { ShouldStartLoadRequest } from 'react-native-webview/lib/WebViewTypes'
import type { TabInfo } from '@signet/shared/types'
import { cosmeticScript, isBlockedUrl } from '@signet/shared/mobile/content-blocking'
import { parseFindMessage, type FindResult } from '@signet/shared/mobile/find'
import { isCancelledLoad } from '@signet/shared/mobile/load-errors'
import { parseReaderMessage, type ReaderArticle } from '@signet/shared/mobile/reader'
import { parseIconsMessage, pickBestIcon } from '@signet/shared/mobile/favicons'
import { parseLinkMenuMessage, type LinkMenuMessage } from '@signet/shared/mobile/link-menu'
import { historyKey } from '@signet/shared/mobile/history'
import { startDownload } from '@/features/library'
import { useContentBlocker } from '../../../modules/signet-web'
import { useStore } from '@/state'
import { useLocalhostAttach } from '@/features/link'
import { usePasswordBridge } from '@/features/passwords'
import { useColors } from '@/ui'
import { ErrorView } from './ErrorView'
import { DESKTOP_USER_AGENT, DESKTOP_VIEWPORT_SCRIPT, LINK_MENU_SCRIPT, META_SCRIPT, SCROLL_SCRIPT, locationScript, zoomScript } from './scripts'
import { patchTabView, useTabView, useViewStore } from './viewStore'

export interface BrowserViewHandle {
  goBack: () => void
  goForward: () => void
  /** Refreshes the page; if the last load failed, retries the failed address. */
  reload: () => void
  stopLoading: () => void
  /** Loads an address in this tab. */
  go: (url: string) => void
  /** Runs a script in the page. */
  inject: (js: string) => void
  /** Fills the login form (Face ID, then account choice if there are several). */
  fillPassword: () => void
}

interface Props {
  tab: TabInfo
  /** Only the displayed tab's WebView reacts to scrolling and messages; the others stay in memory, hidden. */
  visible: boolean
  /** Private tab: ephemeral WebView storage (no shared cookies or cache). */
  isPrivate: boolean
  /** Height at the bottom of the screen the page must not cover (collapsed toolbar). */
  bottomInset: number
  onScroll?: (y: number, max: number) => void
  onFind?: (result: FindResult) => void
  onReader?: (article: ReaderArticle | null) => void
  /** Long press on a link in the page. */
  onLinkMenu?: (link: LinkMenuMessage) => void
}

const parse = (data: string): { type?: string; y?: unknown; max?: unknown; favicon?: unknown } | null => {
  try {
    const v = JSON.parse(data) as unknown
    return v && typeof v === 'object' ? (v as { type?: string }) : null
  } catch {
    return null
  }
}

/**
 * A tab's WebView. The browser keeps a few mounted (the recent tabs): going back to one reloads nothing.
 * The tab's address in the store drives loading: changing it from outside (command bar, start screen) loads the page;
 * navigations inside the page, on the other hand, update the store without reloading (`reported` remembers the last address that came from the page).
 */
export const BrowserView = forwardRef<BrowserViewHandle, Props>(function BrowserView(
  { tab, visible, isPrivate, bottomInset, onScroll, onFind, onReader, onLinkMenu },
  ref
) {
  const c = useColors()
  const web = useRef<WebView<object>>(null)
  const view = useTabView(tab.id)
  const pw = usePasswordBridge(web, tab, isPrivate, visible)
  const blocking = useStore((s) => s.settings.contentBlocking)
  const box = useRef<ComponentRef<typeof View>>(null) // the WebView does not expose a native ref: we go through the surrounding view (the module looks for the WKWebView inside it)
  useContentBlocker(box, blocking) // native WebKit blocking (compiled iOS build); no effect elsewhere
  useLocalhostAttach(box, isPrivate) // localhost relay of the link with the computer: private tabs (separate data store), compiled iOS
  const [source, setSource] = useState(tab.url)
  const sourceRef = useRef(tab.url)
  const reported = useRef(tab.url)
  const mounted = useRef(false)

  const go = useCallback(
    (url: string) => {
      reported.current = url
      patchTabView(tab.id, { error: null, progress: 0 })
      if (url !== sourceRef.current) {
        sourceRef.current = url
        setSource(url)
      } else {
        web.current?.injectJavaScript(locationScript(url))
      }
    },
    [tab.id]
  )

  useImperativeHandle(
    ref,
    () => ({
      goBack: () => web.current?.goBack(),
      goForward: () => web.current?.goForward(),
      reload: () => {
        const failed = useViewStore.getState().byTab[tab.id]?.error?.url
        if (failed) go(failed)
        else web.current?.reload()
      },
      stopLoading: () => web.current?.stopLoading(),
      go,
      inject: (js) => web.current?.injectJavaScript(js),
      fillPassword: pw.fill
    }),
    [go, tab.id, pw.fill]
  )

  // Address changed from outside the browser: load it.
  useEffect(() => {
    if (tab.url && tab.url !== reported.current) go(tab.url)
  }, [tab.url, go])

  // Switching to the desktop version (or back) reloads the page with the other identity.
  useEffect(() => {
    if (!mounted.current) {
      mounted.current = true
      return
    }
    const timer = setTimeout(() => web.current?.reload(), 60)
    return () => clearTimeout(timer)
  }, [view.desktop])

  useEffect(() => {
    web.current?.injectJavaScript(zoomScript(view.zoom))
  }, [view.zoom])

  // Page icon: on the tab, then (outside private browsing) in the history and the page's bookmarks. The icon message may arrive
  // before the visit is written to the history: we hold on to the icon and replay it at the end of loading.
  const pageIcon = useRef<{ url: string; icon: string } | null>(null)
  const applyIcon = (url: string, icon: string) => {
    if (isPrivate) return
    const st = useStore.getState()
    st.setHistoryFavicon(url, icon)
    st.setBookmarkFavicon(url, icon)
  }

  const onNavigationStateChange = (nav: WebViewNavigation) => {
    if (!nav.url || nav.url === 'about:blank') return
    const previous = reported.current
    reported.current = nav.url
    let origin = ''
    let before = ''
    try {
      origin = new URL(nav.url).origin
      before = new URL(previous).origin
    } catch {
      // unreadable address: keep the icon
    }
    useStore.getState().reportNavigation(
      tab.id,
      { url: nav.url, title: nav.title && nav.title !== nav.url ? nav.title : undefined },
      {
        loading: nav.loading,
        canGoBack: nav.canGoBack,
        canGoForward: nav.canGoForward,
        // Other site: the old one's icon must not linger while waiting for the new one's.
        ...(origin && before && origin !== before ? { favicon: undefined } : {})
      }
    )
    const known = pageIcon.current
    if (known && !nav.loading && historyKey(known.url) === historyKey(nav.url)) applyIcon(nav.url, known.icon)
  }

  const onShouldStartLoadWithRequest = (req: ShouldStartLoadRequest) => {
    const url = req.url
    if (/^(about|data|blob|javascript):/i.test(url)) return true
    if (!/^https?:/i.test(url)) {
      // tel:, mailto:, app links...: only for a user tap on the main page.
      if (req.isTopFrame !== false && (req.navigationType === 'click' || Platform.OS !== 'ios')) Linking.openURL(url).catch(() => undefined)
      return false
    }
    return !(blocking && isBlockedUrl(url))
  }

  const onMessage = (e: WebViewMessageEvent) => {
    const data = e.nativeEvent.data
    const msg = parse(data)
    if (!msg) return
    if (msg.type?.startsWith('pw:')) return pw.onMessage(msg)
    if (msg.type === 'icons') {
      const m = parseIconsMessage(data)
      const icon = m ? pickBestIcon(m.icons, m.base) : null
      if (!icon) return
      const page = useStore.getState().workspace.tabs[tab.id]?.url ?? tab.url
      pageIcon.current = { url: page, icon }
      if (icon !== useStore.getState().workspace.tabs[tab.id]?.favicon) useStore.getState().updateTab(tab.id, { favicon: icon })
      applyIcon(page, icon)
    } else if (msg.type === 'linkMenu' && visible) {
      const link = parseLinkMenuMessage(data)
      if (link) onLinkMenu?.(link)
    } else if (msg.type === 'scroll' && visible) {
      if (typeof msg.y === 'number' && typeof msg.max === 'number') onScroll?.(msg.y, msg.max)
    } else if (msg.type === 'find' && visible) {
      const r = parseFindMessage(data)
      if (r) onFind?.(r)
    } else if (msg.type === 'reader' && visible) {
      const r = parseReaderMessage(data)
      if (r) onReader?.(r.article)
    }
  }

  const onOpenWindow = (e: { nativeEvent: { targetUrl: string } }) => {
    const url = e.nativeEvent.targetUrl
    if (!/^https?:/i.test(url) || (blocking && isBlockedUrl(url))) return
    const st = useStore.getState()
    if (st.settings.openLinksInNewTab) st.openTab({ url, private: isPrivate })
    else go(url)
  }

  return (
    <View
      ref={box}
      collapsable={false}
      style={[StyleSheet.absoluteFill, { bottom: bottomInset }, !visible && styles.hidden]}
      pointerEvents={visible ? 'auto' : 'none'}
    >
      <WebView<object>
        ref={web}
        source={{ uri: source }}
        style={[styles.fill, { backgroundColor: c.background }]}
        incognito={isPrivate}
        userAgent={view.desktop ? DESKTOP_USER_AGENT : undefined}
        contentMode={view.desktop ? 'desktop' : 'mobile'}
        textZoom={Platform.OS === 'android' ? view.zoom : undefined}
        originWhitelist={['*']}
        onNavigationStateChange={onNavigationStateChange}
        onShouldStartLoadWithRequest={onShouldStartLoadWithRequest}
        onLoadStart={() => patchTabView(tab.id, { error: null })}
        onLoadProgress={(e) => {
          if (visible) patchTabView(tab.id, { progress: e.nativeEvent.progress })
        }}
        onLoadEnd={() => patchTabView(tab.id, { progress: 1 })}
        onError={(e) => {
          const { code, description, url } = e.nativeEvent
          if (isCancelledLoad(code)) return
          patchTabView(tab.id, { error: { code, description, url: url || tab.url }, progress: 1 })
        }}
        onHttpError={() => undefined}
        onMessage={onMessage}
        onOpenWindow={onOpenWindow}
        onContentProcessDidTerminate={() => web.current?.reload()}
        onScroll={
          Platform.OS === 'ios'
            ? (e: NativeSyntheticEvent<NativeScrollEvent>) => {
                if (!visible) return
                const { contentOffset, contentSize, layoutMeasurement } = e.nativeEvent
                onScroll?.(contentOffset.y, contentSize.height - layoutMeasurement.height)
              }
            : undefined
        }
        onFileDownload={Platform.OS === 'ios' ? (e) => void startDownload(e.nativeEvent.downloadUrl, isPrivate) : undefined}
        injectedJavaScriptBeforeContentLoaded={`${blocking ? cosmeticScript() : ''}${zoomScript(view.zoom)}${LINK_MENU_SCRIPT}${Platform.OS === 'android' && view.desktop ? DESKTOP_VIEWPORT_SCRIPT : ''}`}
        injectedJavaScriptBeforeContentLoadedForMainFrameOnly={false}
        injectedJavaScript={(Platform.OS === 'android' ? META_SCRIPT + SCROLL_SCRIPT : META_SCRIPT) + pw.injectedScript}
        allowsBackForwardNavigationGestures
        allowsLinkPreview={false}
        geolocationEnabled
        thirdPartyCookiesEnabled={!isPrivate}
        allowsFullscreenVideo
        allowsInlineMediaPlayback
        mediaPlaybackRequiresUserAction
        pullToRefreshEnabled
        automaticallyAdjustContentInsets={false}
        contentInsetAdjustmentBehavior="never"
        setSupportMultipleWindows
        sharedCookiesEnabled={!isPrivate}
      />
      {pw.overlay}
      {view.error ? <ErrorView failure={view.error} bottomInset={0} onRetry={() => go(view.error?.url || tab.url)} /> : null}
    </View>
  )
})

const styles = StyleSheet.create({
  fill: { flex: 1 },
  hidden: { display: 'none' }
})
