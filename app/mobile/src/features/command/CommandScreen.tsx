// Command bar: field at the bottom (above the keyboard, within thumb reach) and suggestions rising above it, the best one next to the field.
// A tap outside or a swipe down closes it. Route: app/command.tsx (transparent modal).
import { useEffect, useMemo, useRef, useState, type ComponentRef } from 'react'
import { FlatList, KeyboardAvoidingView, Platform, Pressable, ScrollView, StyleSheet, TextInput, View, useWindowDimensions } from 'react-native'
import * as Clipboard from 'expo-clipboard'
import { useLocalSearchParams, useRouter } from 'expo-router'
import { Gesture, GestureDetector } from 'react-native-gesture-handler'
import Animated, { Easing, runOnJS, useAnimatedStyle, useSharedValue, withSpring, withTiming } from 'react-native-reanimated'
import { useSafeAreaInsets } from 'react-native-safe-area-context'
import { siteOf } from '@signet/shared/site'
import { toUrl } from '@signet/shared/urls'
import { parseSuggestions, resolveInput, searchEngine, suggestionsUrlFor, type SearchEngineId } from '@signet/shared/mobile/search'
import { suggest, withRemoteSuggestions, type Suggestion } from '@signet/shared/mobile/suggestions'
import { visibleTabIds, tabsOf } from '@signet/shared/mobile/workspace'
import { haptic } from '@/lib/haptics'
import { captureCurrentThumbnail } from '@/lib/liveCapture'
import { isLoopbackUrl } from '@signet/shared/link/loopback'
import { resolveLocalhostUrl } from '@/features/link'
import { useT } from '@/lib/i18n'
import { useCurrentTab, usePrivateMode, useSpaces, useStore } from '@/state'
import {
  Favicon,
  GlassBar,
  IconButton,
  PressableScale,
  Text,
  fontStyle,
  gutter,
  icons,
  radius,
  space,
  springs,
  useColors,
  useReduceMotion,
  useTheme
} from '@/ui'

type GroupKey = 'top' | 'tab' | 'bookmark' | 'history' | 'web'
type Row = { type: 'header'; key: string; title: string } | { type: 'paste'; key: string } | { type: 'item'; key: string; s: Suggestion }

const groupOf = (s: Suggestion): GroupKey => (s.kind === 'search' && s.remote ? 'web' : s.kind === 'url' || s.kind === 'search' ? 'top' : s.kind)

/** Engine suggestions for `query`: after 150 ms without typing, cancelable request (1.5 s at most). Never when the setting is off or in private browsing. */
function useRemoteSuggestions(query: string, engine: SearchEngineId, enabled: boolean): string[] {
  const [list, setList] = useState<string[]>([])
  useEffect(() => {
    const q = query.trim()
    if (!enabled || !q || toUrl(q)) {
      setList([])
      return
    }
    const ctrl = new AbortController()
    const timeout = setTimeout(() => ctrl.abort(), 1500)
    const delay = setTimeout(() => {
      fetch(suggestionsUrlFor(engine, q), { signal: ctrl.signal, headers: { Accept: 'application/json' } })
        .then((r) => (r.ok ? (r.json() as Promise<unknown>) : null))
        .then((json) => setList(json ? parseSuggestions(engine, json, q) : []))
        .catch(() => undefined)
        .finally(() => clearTimeout(timeout))
    }, 150)
    return () => {
      clearTimeout(delay)
      clearTimeout(timeout)
      ctrl.abort()
    }
  }, [query, engine, enabled])
  return enabled ? list : []
}

export function CommandScreen() {
  const t = useT()
  const c = useColors()
  const { scheme } = useTheme()
  const router = useRouter()
  const insets = useSafeAreaInsets()
  const { height } = useWindowDimensions()
  const reduce = useReduceMotion()
  const params = useLocalSearchParams<{ newTab?: string; q?: string }>()
  const newTab = params.newTab === '1' || params.newTab === 'true'
  const tab = useCurrentTab()
  const privateMode = usePrivateMode()
  const spaces = useSpaces()
  const activeSpaceId = useStore((s) => s.workspace.activeSpaceId)
  const engine = useStore((s) => s.settings.searchEngine)
  const bookmarks = useStore((s) => s.bookmarks)
  const history = useStore((s) => s.history)
  const workspace = useStore((s) => s.workspace)

  // Initial input: the page's address, selected (replaced with one keystroke). It is not used as a search.
  // DEV: `/command?newTab=1&q=swift` prefills the input (QA screenshots cannot type).
  const initial = newTab ? '' : (tab?.url ?? '')
  const [text, setText] = useState(__DEV__ && params.q ? params.q : initial)
  const input = useRef<ComponentRef<typeof TextInput>>(null)
  const [hasClipboardUrl, setHasClipboardUrl] = useState(false)

  useEffect(() => {
    let alive = true
    // `hasUrlAsync` does not trigger iOS's "Paste from..." warning; the read only happens on tap.
    const check = Platform.OS === 'ios' ? Clipboard.hasUrlAsync() : Clipboard.hasStringAsync()
    void check.then((v) => alive && setHasClipboardUrl(v)).catch(() => undefined)
    const focus = setTimeout(() => input.current?.focus(), 80)
    return () => {
      alive = false
      clearTimeout(focus)
    }
  }, [])

  // Opening: the card rises with a spring; closing: it goes back down before leaving the route.
  const shown = useSharedValue(reduce ? 1 : 0)
  const drag = useSharedValue(0)
  useEffect(() => {
    shown.value = reduce ? withTiming(1, { duration: 0 }) : withSpring(1, springs.default)
  }, [reduce, shown])
  // New tab opened from the tab switcher: once the page is open we return to the browser, not the switcher.
  const opened = useRef(false)
  const leave = () => {
    if (newTab && opened.current) router.dismissAll()
    else if (router.canGoBack()) router.back()
  }
  const dismiss = () => {
    input.current?.blur()
    shown.value = withTiming(0, { duration: reduce ? 0 : 160, easing: Easing.out(Easing.quad) }, (done) => {
      if (done) runOnJS(leave)()
    })
  }
  const cardStyle = useAnimatedStyle(() => ({
    opacity: shown.value,
    transform: [{ translateY: (1 - shown.value) * 40 + drag.value }]
  }))
  const scrimStyle = useAnimatedStyle(() => ({ opacity: shown.value * Math.max(0, 1 - drag.value / 400) }))
  const pan = Gesture.Pan()
    .activeOffsetY(10)
    .failOffsetX([-30, 30])
    .onUpdate((e) => {
      drag.value = Math.max(0, e.translationY)
    })
    .onEnd((e) => {
      if (e.translationY > 70 || e.velocityY > 700) {
        runOnJS(haptic)('light')
        runOnJS(dismiss)()
      } else {
        drag.value = withSpring(0, springs.snappy)
      }
    })

  const query = text === initial ? '' : text
  const wantsRemote = useStore((s) => s.settings.searchSuggestions) && !privateMode
  const remote = useRemoteSuggestions(query, engine, wantsRemote)
  const tabs = useMemo(() => tabsOf(workspace, visibleTabIds(workspace)), [workspace])
  const rows = useMemo<Row[]>(() => {
    const local = suggest(query, { tabs: tabs.filter((tb) => tb.id !== tab?.id || !!query), bookmarks, history: privateMode ? [] : history, engine }, 8)
    const suggestions = withRemoteSuggestions(local, remote, engine, 5)
    // Consecutive groups; displayed bottom to top (the best next to the field), so the header comes after the rows in the inverted list.
    const groups: { key: GroupKey; items: Suggestion[] }[] = []
    for (const s of suggestions) {
      const key = groupOf(s)
      const last = groups[groups.length - 1]
      if (last && last.key === key) last.items.push(s)
      else groups.push({ key, items: [s] })
    }
    const out: Row[] = []
    if (!query && hasClipboardUrl) out.push({ type: 'paste', key: 'paste' })
    groups.forEach((g, gi) => {
      g.items.forEach((s, i) => out.push({ type: 'item', key: `${gi}:${i}:${s.url}`, s }))
      if (g.key !== 'top') {
        const title =
          g.key === 'tab'
            ? t('command.openTabs')
            : g.key === 'bookmark'
              ? t('command.bookmarks')
              : g.key === 'web'
                ? t('command.suggestions')
                : t('command.history')
        out.push({ type: 'header', key: `h:${gi}`, title })
      }
    })
    return out
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `t` follows the language; the rest is listed
  }, [query, tabs, bookmarks, history, engine, privateMode, hasClipboardUrl, tab?.id, remote])

  // Local address (`localhost`, `127.0.0.1`...): with a connected computer it goes through it (see features/link).
  const open = (raw: string) => {
    if (!isLoopbackUrl(raw)) return openResolved(raw)
    void resolveLocalhostUrl(raw).then((url) => {
      if (url) openResolved(url)
    })
  }
  const openResolved = (url: string) => {
    const st = useStore.getState()
    if (newTab || !tab) {
      opened.current = true
      // The displayed tab is being left: snapshot it first for the tab switcher.
      void captureCurrentThumbnail().then(() => {
        useStore.getState().openTab({ url })
        dismiss()
      })
    } else {
      st.updateTab(tab.id, { url, loading: true })
      dismiss()
    }
    haptic('light')
  }
  const submit = () => {
    const url = resolveInput(text, engine)
    if (url) open(url)
  }
  const pick = (s: Suggestion) => {
    if (s.kind === 'tab') {
      opened.current = true
      haptic('selection')
      void captureCurrentThumbnail().then(() => {
        useStore.getState().activateTab(s.tabId)
        dismiss()
      })
    } else open(s.url)
  }
  const pasteAndGo = async () => {
    const pasted = Platform.OS === 'ios' ? ((await Clipboard.getUrlAsync()) ?? '') : await Clipboard.getStringAsync()
    const url = resolveInput(pasted, engine)
    if (url) open(url)
  }

  const bottom = Math.max(insets.bottom, space.sm)

  return (
    <View style={styles.fill}>
      <Animated.View style={[StyleSheet.absoluteFill, { backgroundColor: c.scrim }, scrimStyle]}>
        <Pressable style={styles.fill} onPress={dismiss} accessibilityRole="button" accessibilityLabel={t('common.cancel')} />
      </Animated.View>
      {/* Android draws edge to edge: the window no longer shrinks under the keyboard, so we must offset ourselves. */}
      <KeyboardAvoidingView behavior="padding" style={styles.host} pointerEvents="box-none">
        <Animated.View style={[styles.cardWrap, { paddingTop: insets.top + space.lg, paddingBottom: space.sm }, cardStyle]} pointerEvents="box-none">
          {rows.length > 0 ? (
            <GlassBar floating style={[styles.list, { maxHeight: height * 0.5 }]}>
              <FlatList
                inverted
                data={rows}
                keyExtractor={(r) => r.key}
                keyboardShouldPersistTaps="always"
                showsVerticalScrollIndicator={false}
                contentContainerStyle={styles.listContent}
                renderItem={({ item }) =>
                  item.type === 'header' ? (
                    <Text variant="caption" color="secondary" weight="600" style={styles.header}>
                      {item.title.toUpperCase()}
                    </Text>
                  ) : item.type === 'paste' ? (
                    <PressableScale accessibilityRole="button" onPress={() => void pasteAndGo()} style={styles.row} scaleTo={0.985}>
                      <View style={[styles.glyph, { backgroundColor: c.selection }]}>
                        <icons.ClipboardPaste size={17} color={c.accent} strokeWidth={2} />
                      </View>
                      <Text variant="body" weight="500" style={{ color: c.accent }}>
                        {t('command.pasteAndGo')}
                      </Text>
                    </PressableScale>
                  ) : (
                    <SuggestionRow s={item.s} engineName={searchEngine(engine).name} onPress={() => pick(item.s)} />
                  )
                }
              />
            </GlassBar>
          ) : (
            <View style={styles.list} />
          )}
          <GestureDetector gesture={pan}>
            <GlassBar floating style={[styles.panel, { marginBottom: Platform.OS === 'ios' ? 0 : bottom }]}>
              <View style={[styles.grab, { backgroundColor: c.separator }]} />
              {newTab ? (
                <Text variant="caption" color="secondary" weight="600" align="center" style={styles.newLabel}>
                  {t('command.newTabTitle').toUpperCase()}
                </Text>
              ) : null}
              {!privateMode && spaces.length > 1 ? (
                <ScrollView
                  horizontal
                  showsHorizontalScrollIndicator={false}
                  keyboardShouldPersistTaps="always"
                  contentContainerStyle={styles.spaces}
                  accessibilityLabel={t('command.spaces')}
                >
                  {spaces.map((sp) => {
                    const active = sp.id === activeSpaceId
                    return (
                      <PressableScale
                        key={sp.id}
                        accessibilityRole="button"
                        accessibilityState={{ selected: active }}
                        accessibilityLabel={sp.name}
                        haptic="selection"
                        onPress={() => useStore.getState().setActiveSpace(sp.id)}
                        style={[styles.chip, { backgroundColor: active ? c.selection : c.surface2 }]}
                      >
                        <Text variant="subhead">{sp.icon}</Text>
                        <Text variant="footnote" weight={active ? '600' : '400'} color={active ? 'accent' : 'secondary'} numberOfLines={1}>
                          {sp.name}
                        </Text>
                      </PressableScale>
                    )
                  })}
                </ScrollView>
              ) : null}
              <View style={styles.fieldRow}>
                <View style={[styles.field, { backgroundColor: c.surface2 }]}>
                  {privateMode ? (
                    <icons.VenetianMask size={18} color={c.accent} strokeWidth={2} />
                  ) : (
                    <icons.Search size={18} color={c.textTertiary} strokeWidth={2} />
                  )}
                  <TextInput
                    ref={input}
                    value={text}
                    onChangeText={setText}
                    onSubmitEditing={submit}
                    autoFocus
                    selectTextOnFocus
                    autoCapitalize="none"
                    autoCorrect={false}
                    keyboardType="web-search"
                    returnKeyType="go"
                    enablesReturnKeyAutomatically
                    placeholder={t('command.placeholder')}
                    placeholderTextColor={c.textTertiary}
                    keyboardAppearance={scheme}
                    selectionColor={c.accent}
                    accessibilityLabel={t('command.placeholder')}
                    style={[styles.input, fontStyle('body'), { color: c.text }]}
                  />
                  {text ? <IconButton icon={icons.X} size={16} tone="muted" label={t('browser.clear')} onPress={() => setText('')} /> : null}
                </View>
              </View>
            </GlassBar>
          </GestureDetector>
        </Animated.View>
      </KeyboardAvoidingView>
    </View>
  )
}

function SuggestionRow({ s, engineName, onPress }: { s: Suggestion; engineName: string; onPress: () => void }) {
  const t = useT()
  const c = useColors()
  let title: string
  let subtitle: string
  let lead: React.ReactNode
  switch (s.kind) {
    case 'url':
      title = siteOf(s.url) || s.url
      subtitle = t('command.openAddress')
      lead = (
        <View style={[styles.glyph, { backgroundColor: c.surface2 }]}>
          <icons.Globe size={17} color={c.textSecondary} strokeWidth={2} />
        </View>
      )
      break
    case 'search':
      title = s.query
      subtitle = t('command.searchWith', { engine: engineName })
      lead = (
        <View style={[styles.glyph, { backgroundColor: c.surface2 }]}>
          <icons.Search size={17} color={c.textSecondary} strokeWidth={2} />
        </View>
      )
      break
    case 'tab':
      title = s.title
      subtitle = t('command.switchToTab')
      lead = <Favicon uri={s.favicon} url={s.url} size={32} />
      break
    default:
      title = s.title
      subtitle = siteOf(s.url) || s.url
      lead = <Favicon uri={s.favicon} url={s.url} size={32} />
  }
  return (
    <PressableScale accessibilityRole="button" accessibilityLabel={`${title}, ${subtitle}`} onPress={onPress} style={styles.row} scaleTo={0.985}>
      {lead}
      <View style={styles.rowText}>
        <Text variant="body" numberOfLines={1}>
          {title}
        </Text>
        <Text variant="footnote" color="secondary" numberOfLines={1}>
          {subtitle}
        </Text>
      </View>
      {s.kind === 'tab' ? <icons.ArrowRight size={16} color={c.textTertiary} strokeWidth={2} /> : null}
    </PressableScale>
  )
}

const styles = StyleSheet.create({
  fill: { flex: 1 },
  host: { flex: 1, justifyContent: 'flex-end' },
  cardWrap: { flex: 1, justifyContent: 'flex-end', paddingHorizontal: gutter - 4, gap: space.sm },
  list: { flexShrink: 1 },
  listContent: { paddingVertical: space.xs },
  header: { paddingHorizontal: space.lg, paddingTop: space.md, paddingBottom: space.xs, letterSpacing: 0.3 },
  row: { flexDirection: 'row', alignItems: 'center', gap: space.md, paddingHorizontal: space.lg, minHeight: 52, paddingVertical: space.xs },
  rowText: { flex: 1 },
  glyph: { width: 32, height: 32, borderRadius: radius.tile, alignItems: 'center', justifyContent: 'center' },
  panel: { paddingHorizontal: space.sm, paddingBottom: space.sm },
  grab: { alignSelf: 'center', width: 36, height: 4, borderRadius: 2, marginVertical: space.sm },
  newLabel: { paddingBottom: space.xs, letterSpacing: 0.4 },
  spaces: { gap: space.sm, paddingHorizontal: space.xs, paddingBottom: space.sm },
  chip: { flexDirection: 'row', alignItems: 'center', gap: space.xs, height: 32, paddingHorizontal: space.md, borderRadius: radius.pill },
  fieldRow: { flexDirection: 'row' },
  field: {
    flex: 1,
    height: 48,
    flexDirection: 'row',
    alignItems: 'center',
    gap: space.sm,
    paddingLeft: space.md,
    paddingRight: space.xs,
    borderRadius: radius.control
  },
  input: { flex: 1, height: 48 }
})
