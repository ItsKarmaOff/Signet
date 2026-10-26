// Settings: iOS-style grouped sections (general, appearance, language, privacy and security, bookmarks, about).
import { Linking, Platform, View } from 'react-native'
import { useRouter } from 'expo-router'
import { LOCALES, type UiLanguage } from '@signet/shared/i18n'
import { searchEngine } from '@signet/shared/mobile/search'
import { siteOf } from '@signet/shared/site'
import type { Theme } from '@signet/shared/types'
import { useActiveComputers } from '@/features/link'
import { useT } from '@/lib/i18n'
import { useStore } from '@/state'
import { ListRow, ListSection, Screen, SegmentedControl, SwitchRow, icons, space } from '@/ui'
import { ThemePreview } from './ThemePreview'
import { appIconSupport } from './appIcon'
import { useBiometrics } from './useBiometrics'

export function SettingsScreen() {
  const t = useT()
  const router = useRouter()
  const settings = useStore((s) => s.settings)
  const setSettings = useStore((s) => s.setSettings)
  const biometrics = useBiometrics()
  const computers = useStore((s) => s.links.length)
  const connected = useActiveComputers().length
  const iconsAvailable = appIconSupport() === 'ready'
  const lockUnavailable = biometrics === false
  // Same settings, different vocabulary: iPhone / Face ID on one side, Android / fingerprint on the other.
  const android = Platform.OS === 'android'
  const os = (key: string) => t(android ? `${key}Android` : key)
  const languages: { value: UiLanguage; label: string }[] = [
    { value: 'system', label: t('settings.languageSystem') },
    ...LOCALES.map((l) => ({ value: l.id, label: l.name }))
  ]
  const homepageValue =
    settings.homepage === 'custom'
      ? siteOf(settings.homepageUrl) || settings.homepageUrl
      : settings.homepage === 'blank'
        ? t('settings.homepageBlank')
        : t('settings.homepageStart')

  return (
    <Screen scroll contentStyle={{ paddingTop: space.lg, paddingBottom: space.huge }}>
      <ListSection title={t('settings.general')}>
        <ListRow
          title={t('settings.searchEngine')}
          icon={icons.Search}
          iconColor="#3a82f7"
          value={searchEngine(settings.searchEngine).name}
          onPress={() => router.push('/settings/search-engine')}
        />
        <ListRow title={t('settings.homepage')} icon={icons.Home} iconColor="#34a56f" value={homepageValue} onPress={() => router.push('/settings/homepage')} />
        <SwitchRow
          title={t('settings.openLinksInNewTab')}
          icon={icons.SquareStack}
          iconColor="#f59e0b"
          value={settings.openLinksInNewTab}
          onValueChange={(openLinksInNewTab) => setSettings({ openLinksInNewTab })}
        />
        <SwitchRow
          title={t('settings.haptics')}
          icon={icons.Sparkles}
          iconColor="#af52de"
          value={settings.haptics}
          onValueChange={(haptics) => setSettings({ haptics })}
        />
      </ListSection>

      <ListSection title={t('settings.appearance')}>
        <View style={{ padding: space.md, gap: space.md }}>
          <ThemePreview choice={settings.theme} />
          <SegmentedControl<Theme>
            options={[
              { value: 'system', label: t('settings.themeSystem'), icon: icons.Monitor },
              { value: 'light', label: t('settings.themeLight'), icon: icons.Sun },
              { value: 'dark', label: t('settings.themeDark'), icon: icons.Moon }
            ]}
            value={settings.theme}
            onChange={(theme) => setSettings({ theme })}
          />
        </View>
        <ListRow
          title={t('settings.appIcon')}
          subtitle={iconsAvailable ? undefined : t('settings.appIconUnavailable')}
          icon={icons.Palette}
          iconColor="#ec4899"
          value={iconsAvailable ? t(`appIcons.${settings.appIcon}`) : undefined}
          disabled={!iconsAvailable}
          onPress={() => router.push('/settings/app-icon')}
        />
      </ListSection>

      <ListSection title={t('settings.language')} footer={os('settings.languageFooter')}>
        {languages.map((l) => (
          <ListRow key={l.value} title={l.label} selected={settings.language === l.value} onPress={() => setSettings({ language: l.value })} />
        ))}
      </ListSection>

      <ListSection
        title={t('settings.privacyAndSecurity')}
        footer={`${t('settings.contentBlockingFooter')}${lockUnavailable ? `\n\n${os('settings.privateLockUnavailable')}` : ''}`}
      >
        <SwitchRow
          title={t('settings.contentBlocking')}
          subtitle={t('settings.contentBlockingHint')}
          icon={icons.Shield}
          iconColor="#3a82f7"
          value={settings.contentBlocking}
          onValueChange={(contentBlocking) => setSettings({ contentBlocking })}
        />
        <SwitchRow
          title={t('settings.privateLock')}
          subtitle={os('settings.privateLockHint')}
          icon={icons.Lock}
          iconColor="#8e8e93"
          value={!lockUnavailable && settings.privateLock}
          disabled={biometrics !== true}
          onValueChange={(privateLock) => setSettings({ privateLock })}
        />
        <ListRow title={t('passwords.title')} icon={icons.Key} iconColor="#f59e0b" onPress={() => router.push('/settings/passwords')} />
        <ListRow title={t('settings.data')} icon={icons.Trash} iconColor="#ef4444" onPress={() => router.push('/settings/data')} />
      </ListSection>

      <ListSection>
        <ListRow
          title={t('link.title')}
          icon={icons.Laptop}
          iconColor="#3a82f7"
          value={computers === 0 ? undefined : connected > 0 ? t('link.connectedCount', { count: connected }) : t('link.pairedCount', { count: computers })}
          onPress={() => router.push('/settings/computers')}
        />
      </ListSection>

      <ListSection footer={os('settings.systemSettingsFooter')}>
        <ListRow title={os('settings.systemSettings')} icon={icons.Settings} iconColor="#8e8e93" onPress={() => void Linking.openSettings()} />
      </ListSection>

      <ListSection>
        <ListRow title={t('settings.importExport')} icon={icons.Download} iconColor="#34a56f" onPress={() => router.push('/settings/transfer')} />
        <ListRow title={t('settings.about')} icon={icons.Info} iconColor="#8e8e93" onPress={() => router.push('/settings/about')} />
      </ListSection>
    </Screen>
  )
}
