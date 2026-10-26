// Reader mode: the article extracted from the page, reformatted in a separate WebView (JavaScript off), with size, theme and font.
import { useMemo } from 'react'
import { ActivityIndicator, StyleSheet, View } from 'react-native'
import { WebView } from 'react-native-webview'
import Animated, { FadeIn, FadeOut } from 'react-native-reanimated'
import {
  READER_MAX_SIZE,
  READER_MIN_SIZE,
  READER_PALETTE,
  READER_SIZE_STEP,
  buildReaderHtml,
  defaultReaderPrefs,
  readingMinutes,
  type ReaderArticle,
  type ReaderPrefs,
  type ReaderTheme
} from '@signet/shared/mobile/reader'
import { useT } from '@/lib/i18n'
import { EmptyState, GlassBar, IconButton, PressableScale, SegmentedControl, Text, icons, space, useTheme } from '@/ui'
import { useViewStore } from './viewStore'

export type ReaderState = { status: 'loading' } | { status: 'empty' } | { status: 'ready'; article: ReaderArticle }

interface Props {
  state: ReaderState
  topInset: number
  /** Height by which the container is pushed down by the safe area (0 when it already covers it). */
  bleed?: number
  bottomInset: number
  onClose: () => void
  /** Link tapped in the article: leave reader mode and open it in the tab. */
  onOpenLink: (url: string) => void
}

export function ReaderOverlay({ state, topInset, bleed = topInset, bottomInset, onClose, onOpenLink }: Props) {
  const t = useT()
  const { scheme } = useTheme()
  const stored = useViewStore((s) => s.readerPrefs)
  const setPrefs = useViewStore((s) => s.setReaderPrefs)
  const prefs: ReaderPrefs = stored ?? defaultReaderPrefs(scheme)
  const palette = READER_PALETTE[prefs.theme]
  const html = useMemo(
    () => (state.status === 'ready' ? buildReaderHtml(state.article, prefs, t('reader.minutes', { count: readingMinutes(state.article.length) })) : ''),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `t` changes with the language, already reflected by the labels
    [state, prefs.size, prefs.theme, prefs.serif]
  )
  const patch = (p: Partial<ReaderPrefs>) => setPrefs({ ...prefs, ...p })

  return (
    <Animated.View
      entering={FadeIn.duration(180)}
      exiting={FadeOut.duration(140)}
      style={[StyleSheet.absoluteFill, { backgroundColor: palette.bg, top: -bleed, bottom: bottomInset }]}
    >
      <GlassBar edge="bottom" style={[styles.top, { paddingTop: topInset }]}>
        <View style={styles.topRow}>
          <IconButton icon={icons.X} label={t('reader.close')} onPress={onClose} />
          <Text variant="headline" style={styles.title} numberOfLines={1} align="center">
            {t('reader.title')}
          </Text>
          <View style={styles.sizes}>
            <IconButton
              icon={icons.Minus}
              size={19}
              label={t('reader.smaller')}
              disabled={prefs.size <= READER_MIN_SIZE}
              onPress={() => patch({ size: prefs.size - READER_SIZE_STEP })}
            />
            <IconButton
              icon={icons.Plus}
              size={19}
              label={t('reader.larger')}
              disabled={prefs.size >= READER_MAX_SIZE}
              onPress={() => patch({ size: prefs.size + READER_SIZE_STEP })}
            />
          </View>
        </View>
        <View style={styles.themeRow}>
          <View style={styles.segment}>
            <SegmentedControl<ReaderTheme>
              options={[
                { value: 'light', label: t('reader.light') },
                { value: 'sepia', label: t('reader.sepia') },
                { value: 'dark', label: t('reader.dark') }
              ]}
              value={prefs.theme}
              onChange={(theme) => patch({ theme })}
            />
          </View>
          <PressableScale
            accessibilityRole="button"
            accessibilityLabel={prefs.serif ? t('reader.sans') : t('reader.serif')}
            haptic="selection"
            onPress={() => patch({ serif: !prefs.serif })}
            style={styles.font}
          >
            <icons.TextSize size={prefs.serif ? 22 : 19} color={palette.text} strokeWidth={prefs.serif ? 2.4 : 1.8} />
          </PressableScale>
        </View>
      </GlassBar>
      {state.status === 'ready' ? (
        <WebView
          key={html.length}
          source={{ html }}
          style={{ flex: 1, backgroundColor: palette.bg }}
          javaScriptEnabled={false}
          originWhitelist={['*']}
          onShouldStartLoadWithRequest={(req) => {
            if (/^https?:/i.test(req.url)) {
              onOpenLink(req.url)
              return false
            }
            return true
          }}
          allowsLinkPreview
          setSupportMultipleWindows={false}
        />
      ) : state.status === 'loading' ? (
        <View style={styles.center}>
          <ActivityIndicator color={palette.secondary} />
          <Text variant="subhead" style={{ color: palette.secondary }}>
            {t('reader.loading')}
          </Text>
        </View>
      ) : (
        <View style={styles.center}>
          <EmptyState
            icon={icons.BookOpen}
            title={t('reader.emptyTitle')}
            message={t('reader.emptyMessage')}
            action={{ label: t('reader.close'), onPress: onClose }}
          />
        </View>
      )}
    </Animated.View>
  )
}

const styles = StyleSheet.create({
  top: { paddingBottom: space.sm },
  topRow: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: space.sm },
  title: { flex: 1 },
  sizes: { flexDirection: 'row' },
  themeRow: { flexDirection: 'row', alignItems: 'center', gap: space.md, paddingHorizontal: space.lg },
  segment: { flex: 1 },
  font: { width: 48, height: 40, alignItems: 'center', justifyContent: 'center' },
  center: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: space.md }
})
