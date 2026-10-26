// Content of a switcher page: favorites (row of icons), pinned, then today's tabs in a grid of cards.
import { ScrollView, StyleSheet, View, useWindowDimensions } from 'react-native'
import type { TabInfo } from '@signet/shared/types'
import { useT } from '@/lib/i18n'
import { EmptyState, Text, gutter, icons, space } from '@/ui'
import { FavoriteTile, TabCard } from './TabCard'

export interface TabSectionsProps {
  favorites: TabInfo[]
  pinned: TabInfo[]
  today: TabInfo[]
  /** Tab to highlight (the one shown in the browser); `null` if this page is not the active page. */
  selectedId: string | null
  /** Page width. */
  width: number
  /** Page height (required in the horizontal scrolling between spaces). */
  height?: number
  topInset?: number
  bottomInset: number
  onOpen: (id: string) => void
  onClose: (id: string) => void
  onMenu: (id: string) => void
  onTouch: (active: boolean) => void
}

const GAP = space.md

export function TabSections({
  favorites,
  pinned,
  today,
  selectedId,
  width,
  height,
  topInset = 0,
  bottomInset,
  onOpen,
  onClose,
  onMenu,
  onTouch
}: TabSectionsProps) {
  const t = useT()
  const { width: screen } = useWindowDimensions()
  const inner = Math.min(width, screen) - gutter * 2
  const cols = Math.max(2, Math.floor((inner + GAP) / (170 + GAP)))
  const cardW = Math.floor((inner - GAP * (cols - 1)) / cols)
  const favCols = Math.max(4, Math.floor((inner + GAP) / (72 + GAP)))
  const favW = Math.floor((inner - GAP * (favCols - 1)) / favCols)
  const headed = favorites.length > 0 || pinned.length > 0
  const total = favorites.length + pinned.length + today.length

  const grid = (tabs: TabInfo[], closable: boolean) => (
    <View style={styles.grid}>
      {tabs.map((tab) => (
        <TabCard
          key={tab.id}
          tab={tab}
          width={cardW}
          selected={tab.id === selectedId}
          closable={closable}
          onOpen={onOpen}
          onClose={onClose}
          onMenu={onMenu}
          onTouch={onTouch}
        />
      ))}
    </View>
  )

  return (
    <ScrollView
      style={height === undefined ? { width, flex: 1 } : { width, height }}
      contentContainerStyle={[styles.content, { paddingTop: topInset + space.sm, paddingBottom: bottomInset }]}
      showsVerticalScrollIndicator={false}
      alwaysBounceVertical
    >
      <View style={{ paddingHorizontal: gutter }}>
        {total === 0 ? <EmptyState icon={icons.SquareStack} title={t('tabs.empty')} message={t('tabs.emptyHint')} /> : null}
        {favorites.length > 0 ? (
          <View style={styles.section}>
            <Header label={t('tabs.favorites')} />
            <View style={styles.grid}>
              {favorites.map((tab) => (
                <FavoriteTile key={tab.id} tab={tab} width={favW} selected={tab.id === selectedId} onOpen={onOpen} onMenu={onMenu} />
              ))}
            </View>
          </View>
        ) : null}
        {pinned.length > 0 ? (
          <View style={styles.section}>
            <Header label={t('tabs.pinned')} />
            {grid(pinned, false)}
          </View>
        ) : null}
        {today.length > 0 ? (
          <View style={styles.section}>
            {headed ? <Header label={t('tabs.today')} /> : null}
            {grid(today, true)}
          </View>
        ) : null}
      </View>
    </ScrollView>
  )
}

function Header({ label }: { label: string }) {
  return (
    <Text variant="footnote" color="secondary" accessibilityRole="header" style={styles.header}>
      {label.toUpperCase()}
    </Text>
  )
}

const styles = StyleSheet.create({
  content: { flexGrow: 1 },
  section: { marginBottom: space.xl },
  header: { marginLeft: space.xs, marginBottom: space.sm, letterSpacing: 0.2 },
  grid: { flexDirection: 'row', flexWrap: 'wrap', gap: GAP }
})
