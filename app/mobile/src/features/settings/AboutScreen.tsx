import { StyleSheet, View } from 'react-native'
import Constants from 'expo-constants'
import { Image } from 'expo-image'
import * as WebBrowser from 'expo-web-browser'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ListRow, ListSection, Screen, Text, icons, radius, space, toast } from '@/ui'
import { APP_ICON_IMAGES } from './appIconImages'

const REPO = 'https://github.com/ItsKarmaOff/Signet'

export function AboutScreen() {
  const t = useT()
  const appIcon = useStore((s) => s.settings.appIcon)
  const version = Constants.expoConfig?.version ?? '0.1.0'
  const openSource = () => WebBrowser.openBrowserAsync(REPO).catch(() => toast.show({ message: t('about.sourceError') }))
  return (
    <Screen scroll contentStyle={{ paddingTop: space.xl, paddingBottom: space.huge }}>
      <View style={styles.hero}>
        <Image source={APP_ICON_IMAGES[appIcon]} style={styles.icon} accessibilityIgnoresInvertColors />
        <Text variant="title2">Signet</Text>
        <Text variant="subhead" color="secondary" align="center" style={styles.tagline}>
          {t('about.tagline')}
        </Text>
      </View>
      <ListSection>
        <ListRow title={t('about.version')} icon={icons.Info} iconColor="#8e8e93" value={version} />
        <ListRow title={t('about.licence')} icon={icons.Scale} iconColor="#3a82f7" value={t('about.licenceValue')} />
        <ListRow
          title={t('about.source')}
          subtitle={t('about.sourceHint')}
          icon={icons.Code}
          iconColor="#34a56f"
          right={<icons.ExternalLink size={18} color="#8e8e93" strokeWidth={2} />}
          onPress={openSource}
        />
      </ListSection>
    </Screen>
  )
}

const styles = StyleSheet.create({
  hero: { alignItems: 'center', gap: space.sm, marginBottom: space.xxl, paddingHorizontal: space.xxl },
  icon: { width: 88, height: 88, borderRadius: radius.card, marginBottom: space.sm },
  tagline: { maxWidth: 280 }
})
