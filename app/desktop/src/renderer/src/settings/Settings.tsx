import { Fragment, useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { Trans, useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { InlineConfirm } from '../ui/InlineConfirm'
import { LOCALES, resolveLocale } from '@signet/shared/i18n'
import type { Locale, UiLanguage } from '@signet/shared/i18n'
import {
  ArchiveRestore,
  Check,
  Info,
  Signpost,
  Palette,
  Rocket,
  Download,
  Settings as Cog,
  Shield,
  KeyRound,
  Keyboard,
  Trash2,
  Pencil,
  Puzzle,
  Paintbrush,
  AppWindow,
  Smartphone,
  SquareTerminal,
  Cpu,
  ExternalLink,
  RotateCw
} from 'lucide-react'
import type { BrowsingDataKind, DevAlias, ImportKind, SettingsInfo } from '@shared/types'
import { aliasTarget, isAliasName } from '@shared/devhosts'
import { DEFAULT_FONT_SIZE, DEFAULT_ZOOM, FONT_SIZES, MIN_FONT_SIZES, ZOOM_PERCENTS } from '@shared/pagesettings'
import { useHue } from '../lib/util'
import { RoutingSettings } from './RoutingSettings'
import { DEFAULT_SEARCH_ENGINE, SEARCH_ENGINES, type SearchEngineId } from '@signet/shared/mobile/search'
import { LanguageSettings } from './LanguageSettings'
import { ExtensionSettings } from './ExtensionSettings'
import { ShortcutSettings } from './ShortcutSettings'
import { LiveFolderSettings } from './LiveFolderSettings'
import { PasswordSettings } from './PasswordSettings'
import { BackupSettings } from './BackupSettings'
import { SitePermissionSettings } from './SitePermissionSettings'
import { IS_MAC } from '@signet/shared/i18n'
import { shortcutLabel } from '@shared/shortcuts'
import { ModSettings } from './ModSettings'
import { WebAppSettings } from './WebAppSettings'
import { DeviceSettings } from './DeviceSettings'
import { DNS_PROVIDERS, DEFAULT_DNS_SERVER, isDohTemplate } from '@shared/dns'

const REPO = 'https://github.com/ItsKarmaOff/Signet'

const SECTIONS = [
  { id: 'general', group: 'basics', label: 'settings.section.general', icon: Cog },
  { id: 'appearance', group: 'basics', label: 'settings.section.appearance', icon: Palette },
  { id: 'shortcuts', group: 'basics', label: 'settings.section.shortcuts', icon: Keyboard },
  { id: 'boosts', group: 'browsing', label: 'settings.section.boosts', icon: Rocket },
  { id: 'routing', group: 'browsing', label: 'settings.section.routing', icon: Signpost },
  { id: 'extensions', group: 'browsing', label: 'settings.section.extensions', icon: Puzzle },
  // Sites installed as apps: macOS launcher, so section reserved for Mac.
  { id: 'apps', group: 'browsing', label: 'settings.section.apps', icon: AppWindow, macOnly: true },
  { id: 'devices', group: 'browsing', label: 'settings.section.devices', icon: Smartphone },
  { id: 'privacy', group: 'data', label: 'settings.section.privacy', icon: Shield },
  { id: 'passwords', group: 'data', label: 'settings.section.passwords', icon: KeyRound },
  { id: 'backup', group: 'data', label: 'settings.section.backup', icon: ArchiveRestore },
  { id: 'import', group: 'data', label: 'settings.section.import', icon: Download },
  { id: 'mods', group: 'advanced', label: 'settings.section.mods', icon: Paintbrush },
  { id: 'developer', group: 'advanced', label: 'settings.section.developer', icon: SquareTerminal },
  { id: 'system', group: 'advanced', label: 'settings.section.system', icon: Cpu },
  { id: 'about', group: 'advanced', label: 'settings.section.about', icon: Info }
] as const
type SectionId = (typeof SECTIONS)[number]['id']

const ARCHIVE_CHOICES = [6, 12, 24, 72, 168, 0]
/** Sleep delays offered, in minutes (0 = never). */
const SLEEP_CHOICES = [5, 15, 30, 60, 120, 360, 0]

function sleepLabel(t: TFunction, minutes: number): string {
  if (minutes === 0) return t('settings.general.sleep.never')
  return minutes % 60 === 0 ? t('settings.general.sleep.hours', { count: minutes / 60 }) : t('settings.general.sleep.minutes', { count: minutes })
}

function archiveLabel(t: TFunction, hours: number): string {
  if (hours === 0) return t('settings.general.archive.never')
  return hours % 24 === 0 ? t('settings.general.archive.days', { count: hours / 24 }) : t('settings.general.archive.hours', { count: hours })
}

/** Route #settings: settings window. */
export function Settings() {
  const { t } = useTranslation()
  const [info, setInfo] = useState<SettingsInfo | null>(null)
  const [section, setSection] = useState<SectionId>('general')
  useHue(info?.hue)

  useEffect(() => {
    let alive = true
    const off = window.signet.onSettingsInfo(setInfo)
    // A pushed state is always more recent than the initial read.
    window.signet
      .getSettingsInfo()
      .then((i) => alive && setInfo((prev) => prev ?? i))
      .catch(() => {})
    return () => {
      alive = false
      off()
    }
  }, [])

  const current = SECTIONS.find((s) => s.id === section)!
  return (
    <div className="settings">
      <nav className="st-nav">
        <div className="st-nav-top" />
        {SECTIONS.map((item, i) => {
          if ('macOnly' in item && !IS_MAC) return null
          const { id, label, icon: Icon, group } = item
          return (
            <Fragment key={id}>
              {group !== SECTIONS[i - 1]?.group && <div className="st-nav-group">{t(`settings.group.${group}`)}</div>}
              <button data-section={id} className={`st-nav-item${id === section ? ' on' : ''}`} onClick={() => setSection(id)}>
                <Icon size={15} />
                {t(label)}
              </button>
            </Fragment>
          )
        })}
      </nav>
      <main className="st-main">
        <header className="st-title">{t(current.label)}</header>
        <div className="st-body">
          {info && (section === 'boosts' || section === 'routing') && <p className="st-note">{t(`settings.desc.${section}`)}</p>}
          {info && (
            <>
              {section === 'general' && <General info={info} />}
              {section === 'appearance' && <Appearance info={info} />}
              {section === 'privacy' && <Privacy info={info} />}
              {section === 'passwords' && <PasswordSettings info={info} />}
              {section === 'backup' && <BackupSettings />}
              {section === 'import' && <Import info={info} />}
              {section === 'boosts' && <Boosts info={info} />}
              {section === 'routing' && <RoutingSettings info={info} />}
              {section === 'extensions' && <ExtensionSettings info={info} />}
              {section === 'apps' && <WebAppSettings />}
              {section === 'devices' && <DeviceSettings />}
              {section === 'shortcuts' && <ShortcutSettings info={info} />}
              {section === 'mods' && <ModSettings />}
              {section === 'developer' && <Developer info={info} />}
              {section === 'system' && <System info={info} />}
              {section === 'about' && <About info={info} />}
            </>
          )}
        </div>
      </main>
    </div>
  )
}

// ───────────────────────── Shared elements ─────────────────────────

function Group({ children }: { children: ReactNode }) {
  return <div className="st-group">{children}</div>
}

function Row({ label, hint, children }: { label: string; hint?: string; children?: ReactNode }) {
  return (
    <div className="st-row">
      <div className="st-label">
        <span>{label}</span>
        {hint && <span className="st-hint">{hint}</span>}
      </div>
      {children}
    </div>
  )
}

function Switch({ on, label, onChange }: { on: boolean; label: string; onChange: (on: boolean) => void }) {
  return (
    <button className={`switch ${on ? 'on' : ''}`} role="switch" aria-checked={on} aria-label={label} onClick={() => onChange(!on)}>
      <span className="knob" />
    </button>
  )
}

// ───────────────────────── Sections ─────────────────────────

/** Local names: `mysite` → address of a local server, typed in the command bar and displayed in the address bar. */
function LocalNames({ aliases }: { aliases: DevAlias[] }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [target, setTarget] = useState('')
  const [error, setError] = useState<string | null>(null)
  const save = (devAliases: DevAlias[]) => void window.signet.updateSettings({ devAliases })
  const add = () => {
    const n = name.trim().toLowerCase()
    const url = aliasTarget(target)
    if (!isAliasName(n)) return setError(t('settings.developer.aliasBadName'))
    if (aliases.some((a) => a.name === n)) return setError(t('settings.developer.aliasTaken', { name: n }))
    if (!url) return setError(t('settings.developer.aliasBadTarget'))
    save([...aliases, { name: n, target: url }])
    setName('')
    setTarget('')
    setError(null)
  }
  return (
    <>
      <h3 className="st-sub">{t('settings.developer.aliases')}</h3>
      <p className="st-note">{t('settings.developer.aliasesHint')}</p>
      <Group>
        {aliases.map((a) => (
          <div key={a.name} className="st-row">
            <div className="st-label">
              <span className="st-alias-name">{a.name}</span>
              <span className="st-hint">{a.target}</span>
            </div>
            <button className="st-btn" onClick={() => save(aliases.filter((x) => x.name !== a.name))}>
              <Trash2 size={13} />
              {t('settings.developer.aliasRemove')}
            </button>
          </div>
        ))}
        <form
          className="st-row st-alias-add"
          onSubmit={(e) => {
            e.preventDefault()
            add()
          }}
        >
          <input
            className="st-input"
            value={name}
            placeholder={t('settings.developer.aliasNamePlaceholder')}
            aria-label={t('settings.developer.aliasName')}
            spellCheck={false}
            onChange={(e) => (setName(e.target.value), setError(null))}
          />
          <span className="st-alias-arrow" aria-hidden />
          <input
            className="st-input"
            value={target}
            placeholder={t('settings.developer.aliasTargetPlaceholder')}
            aria-label={t('settings.developer.aliasTarget')}
            spellCheck={false}
            onChange={(e) => (setTarget(e.target.value), setError(null))}
          />
          <button className="st-btn" type="submit" disabled={!name.trim() || !target.trim()}>
            {t('settings.developer.aliasAdd')}
          </button>
        </form>
        {error && (
          <p className="st-note st-err" role="alert">
            {error}
          </p>
        )}
      </Group>
    </>
  )
}

/** Development: developer mode (local pages) and reminder of the tools shortcuts. */
function Developer({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const { settings } = info
  const on = settings.devMode === true
  const keys = (id: string) => shortcutLabel(id, settings.shortcuts, IS_MAC) || '-'
  return (
    <>
      <Group>
        <Row label={t('settings.developer.devMode')} hint={t('settings.developer.devModeHint')}>
          <Switch on={on} label={t('settings.developer.devMode')} onChange={(devMode) => window.signet.updateSettings({ devMode })} />
        </Row>
        {on && (
          <Row label={t('settings.developer.keepOut')} hint={t('settings.developer.keepOutHint')}>
            <Switch
              on={settings.devKeepOutOfHistory === true}
              label={t('settings.developer.keepOut')}
              onChange={(devKeepOutOfHistory) => window.signet.updateSettings({ devKeepOutOfHistory })}
            />
          </Row>
        )}
      </Group>
      <Group>
        <Row label={t('settings.developer.jsonViewer')} hint={t('settings.developer.jsonViewerHint')}>
          <Switch
            on={settings.jsonViewer !== false}
            label={t('settings.developer.jsonViewer')}
            onChange={(jsonViewer) => window.signet.updateSettings({ jsonViewer })}
          />
        </Row>
      </Group>
      <LocalNames aliases={settings.devAliases ?? []} />
      <p className="st-note">
        {t('settings.developer.shortcutsHint', { devtools: keys('devTools'), console: keys('jsConsole'), responsive: keys('responsive') })}
      </p>
    </>
  )
}

/** System: engine settings (graphics acceleration, proxy). */
function System({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const on = info.settings.hardwareAcceleration !== false
  const pending = on !== info.hardwareAccelerationActive
  return (
    <Group>
      <Row
        label={t('settings.system.gpu')}
        hint={pending ? t(info.packaged ? 'settings.system.gpuPending' : 'settings.system.gpuPendingDev') : t('settings.system.gpuHint')}
      >
        {pending && info.packaged && (
          <button className="st-btn" onClick={() => void window.signet.relaunchApp()}>
            <RotateCw size={13} />
            {t('settings.system.relaunch')}
          </button>
        )}
        <Switch on={on} label={t('settings.system.gpu')} onChange={(hardwareAcceleration) => window.signet.updateSettings({ hardwareAcceleration })} />
      </Row>
      {info.proxySettings && (
        <Row label={t('settings.system.proxy')} hint={t('settings.system.proxyHint')}>
          <button className="st-btn" onClick={() => void window.signet.openProxySettings()}>
            <ExternalLink size={13} />
            {t('settings.system.proxyOpen')}
          </button>
        </Row>
      )}
    </Group>
  )
}

const CUSTOM_DNS = 'custom'

/** Privacy › Secure DNS: off, with the current provider, or with a chosen DNS over HTTPS server. */
function SecureDns({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const mode = info.settings.secureDns ?? 'automatic'
  const server = info.settings.secureDnsServer ?? DEFAULT_DNS_SERVER
  const known = DNS_PROVIDERS.find((p) => p.template === server)
  const [picked, setPicked] = useState<string>(known?.id ?? CUSTOM_DNS)
  const [custom, setCustom] = useState(known ? '' : server)
  const customOk = isDohTemplate(custom.trim())
  const choice = mode === 'off' ? 'off' : mode === 'automatic' ? 'automatic' : picked
  const choose = (value: string) => {
    if (value === 'off' || value === 'automatic') return void window.signet.updateSettings({ secureDns: value })
    setPicked(value)
    const p = DNS_PROVIDERS.find((x) => x.id === value)
    if (p) void window.signet.updateSettings({ secureDns: 'secure', secureDnsServer: p.template })
    else if (customOk) void window.signet.updateSettings({ secureDns: 'secure', secureDnsServer: custom.trim() })
  }
  const applyCustom = () => {
    if (customOk) void window.signet.updateSettings({ secureDns: 'secure', secureDnsServer: custom.trim() })
  }
  return (
    <Group>
      <Row label={t('settings.privacy.dns')} hint={t('settings.privacy.dnsHint')}>
        <select className="st-select" value={choice} onChange={(e) => choose(e.target.value)} aria-label={t('settings.privacy.dns')}>
          <option value="automatic">{t('settings.privacy.dnsAutomatic')}</option>
          <option value="off">{t('settings.privacy.dnsOff')}</option>
          {DNS_PROVIDERS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
          <option value={CUSTOM_DNS}>{t('settings.privacy.dnsCustom')}</option>
        </select>
      </Row>
      {choice === CUSTOM_DNS && (
        <form
          className="st-row st-dns-custom"
          onSubmit={(e) => {
            e.preventDefault()
            applyCustom()
          }}
        >
          <input
            className="st-input"
            value={custom}
            placeholder={t('settings.privacy.dnsCustomPlaceholder')}
            aria-label={t('settings.privacy.dnsCustomLabel')}
            aria-invalid={!!custom.trim() && !customOk}
            spellCheck={false}
            onChange={(e) => setCustom(e.target.value)}
          />
          <button className="st-btn" type="submit" disabled={!customOk || (mode === 'secure' && custom.trim() === server)}>
            {t('settings.privacy.dnsUse')}
          </button>
        </form>
      )}
      {choice === CUSTOM_DNS && !!custom.trim() && !customOk && (
        <p className="st-note st-err" role="alert">
          {t('settings.privacy.dnsCustomInvalid')}
        </p>
      )}
    </Group>
  )
}

function General({ info }: { info: SettingsInfo }) {
  const { t, i18n } = useTranslation()
  const signet = window.signet
  const { settings, packaged, defaultBrowser } = info
  const known = ARCHIVE_CHOICES.includes(settings.archiveAfterHours)
  const sleepMinutes = settings.tabSleepMinutes ?? 30
  const pref: UiLanguage = settings.uiLanguage ?? 'system'
  // "System (…)": the language macOS would give (the current one if already following the system).
  const systemLocale = pref === 'system' ? (i18n.language as Locale) : resolveLocale('system', navigator.languages)
  return (
    <>
      <Group>
        <Row label={t('settings.general.uiLanguage')} hint={t('settings.general.uiLanguageHint')}>
          <select className="st-select" value={pref} onChange={(e) => signet.updateSettings({ uiLanguage: e.target.value as UiLanguage })}>
            <option value="system">
              {t('settings.general.uiLanguageSystem', { language: LOCALES.find((l) => l.id === systemLocale)?.name ?? systemLocale })}
            </option>
            {LOCALES.map((l) => (
              <option key={l.id} value={l.id}>
                {l.name}
              </option>
            ))}
          </select>
        </Row>
        <Row label={t('settings.general.searchEngine')} hint={t('settings.general.searchEngineHint')}>
          <select
            className="st-select"
            value={settings.searchEngine ?? DEFAULT_SEARCH_ENGINE}
            onChange={(e) => signet.updateSettings({ searchEngine: e.target.value as SearchEngineId })}
          >
            {SEARCH_ENGINES.map((e) => (
              <option key={e.id} value={e.id}>
                {e.name}
              </option>
            ))}
          </select>
        </Row>
      </Group>
      <Group>
        <Row label={t('settings.general.archiveAfter')} hint={t('settings.general.archiveAfterHint')}>
          <select
            className="st-select"
            value={settings.archiveAfterHours}
            onChange={(e) => signet.updateSettings({ archiveAfterHours: Number(e.target.value) })}
          >
            {!known && <option value={settings.archiveAfterHours}>{archiveLabel(t, settings.archiveAfterHours)}</option>}
            {ARCHIVE_CHOICES.map((h) => (
              <option key={h} value={h}>
                {archiveLabel(t, h)}
              </option>
            ))}
          </select>
        </Row>
        <Row label={t('settings.general.sleepAfter')} hint={t('settings.general.sleepAfterHint')}>
          <select className="st-select" value={sleepMinutes} onChange={(e) => signet.updateSettings({ tabSleepMinutes: Number(e.target.value) })}>
            {!SLEEP_CHOICES.includes(sleepMinutes) && <option value={sleepMinutes}>{sleepLabel(t, sleepMinutes)}</option>}
            {SLEEP_CHOICES.map((m) => (
              <option key={m} value={m}>
                {sleepLabel(t, m)}
              </option>
            ))}
          </select>
        </Row>
        <Row label={t('settings.general.newTabPosition')} hint={t('settings.general.newTabPositionHint')}>
          <Choice
            value={settings.newTabPosition ?? 'top'}
            options={[
              ['top', t('settings.general.newTabTop')],
              ['bottom', t('settings.general.newTabBottom')]
            ]}
            onChange={(newTabPosition) => signet.updateSettings({ newTabPosition })}
          />
        </Row>
        <Row label={t('settings.general.peekLinks')}>
          <Switch on={settings.peekExternalLinks} label={t('settings.general.peekLinks')} onChange={(on) => signet.updateSettings({ peekExternalLinks: on })} />
        </Row>
        <Row label={t('settings.general.swipe')} hint={t('settings.general.swipeHint')}>
          <Switch
            on={settings.swipeNavigation !== false}
            label={t('settings.general.swipe')}
            onChange={(on) => signet.updateSettings({ swipeNavigation: on })}
          />
        </Row>
        <Row label={t('settings.general.pip')} hint={t('settings.general.pipHint')}>
          <Switch on={settings.autoPip} label={t('settings.general.pip')} onChange={(on) => signet.updateSettings({ autoPip: on })} />
        </Row>
        <Row label={t('settings.general.confirmQuit')} hint={t('settings.general.confirmQuitHint')}>
          <Switch on={settings.confirmQuit !== false} label={t('settings.general.confirmQuit')} onChange={(on) => signet.updateSettings({ confirmQuit: on })} />
        </Row>
      </Group>
      <Group>
        <div className="st-row">
          <div className="st-label">
            <span>{t('settings.general.downloadDir')}</span>
            <span className="st-hint st-path">{info.downloadDir}</span>
          </div>
          {settings.downloadDir && (
            <button className="st-btn" onClick={() => signet.updateSettings({ downloadDir: '' })}>
              {t('settings.general.downloadDirReset')}
            </button>
          )}
          <button className="st-btn" onClick={() => void signet.chooseDownloadDir()}>
            {t('settings.general.downloadDirChange')}
          </button>
        </div>
        <Row label={t('settings.general.askDownloadPath')} hint={t('settings.general.askDownloadPathHint')}>
          <Switch
            on={settings.askDownloadPath === true}
            label={t('settings.general.askDownloadPath')}
            onChange={(on) => signet.updateSettings({ askDownloadPath: on })}
          />
        </Row>
      </Group>
      <Group>
        <div className="st-row st-row-stack">
          <div className="st-label">
            <span>{t('settings.general.siteLanguages')}</span>
            <span className="st-hint">{t('settings.general.siteLanguagesHint')}</span>
          </div>
          <LanguageSettings languages={settings.languages} onChange={(languages) => signet.updateSettings({ languages })} />
        </div>
        <Row label={t('settings.general.spellcheck')} hint={t('settings.general.spellcheckHint')}>
          <Switch on={settings.spellcheck !== false} label={t('settings.general.spellcheck')} onChange={(on) => signet.updateSettings({ spellcheck: on })} />
        </Row>
      </Group>
      <Group>
        <Row
          label={t('settings.general.defaultBrowser')}
          hint={packaged ? (defaultBrowser ? t('settings.general.isDefault') : t('settings.general.otherDefault')) : t('settings.general.installedOnly')}
        >
          {defaultBrowser ? (
            <span className="st-ok">
              <Check size={14} strokeWidth={2.4} /> {t('settings.general.default')}
            </span>
          ) : (
            <button className="st-btn" disabled={!packaged} onClick={() => signet.setDefaultBrowser()}>
              {t('settings.general.makeDefault')}
            </button>
          )}
        </Row>
      </Group>
      <LiveFolderSettings />
    </>
  )
}

/** Settings dropdown list: `[value, label]` options. */
function Choice<T extends string>({ value, options, onChange }: { value: T; options: readonly (readonly [T, string])[]; onChange: (v: T) => void }) {
  return (
    <select className="st-select" value={value} onChange={(e) => onChange(e.target.value as T)}>
      {options.map(([v, label]) => (
        <option key={v} value={v}>
          {label}
        </option>
      ))}
    </select>
  )
}

/** Labels of the default font sizes (same order as `FONT_SIZES`). */
const FONT_LABELS = [
  'settings.appearance.fontVerySmall',
  'settings.appearance.fontSmall',
  'settings.appearance.fontMedium',
  'settings.appearance.fontLarge',
  'settings.appearance.fontVeryLarge'
] as const

function Appearance({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const update = window.signet.updateSettings
  const { settings } = info
  const horizontal = settings.tabLayout === 'horizontal'
  // Current bookmarks bar shortcut (editable in Settings › Shortcuts).
  const barKeys = shortcutLabel('bookmarksBar', settings.shortcuts, IS_MAC)
  const layoutKeys = shortcutLabel('tabLayout', settings.shortcuts, IS_MAC)
  const compactKeys = shortcutLabel('compactMode', settings.shortcuts, IS_MAC)
  return (
    <>
      <h3 className="st-sub">{t('settings.appearance.groupLayout')}</h3>
      <Group>
        <Row
          label={t('settings.appearance.layout')}
          hint={layoutKeys ? t('settings.appearance.layoutHint', { keys: layoutKeys }) : t('settings.appearance.layoutHintNoKeys')}
        >
          <Choice
            value={settings.tabLayout ?? 'vertical'}
            options={[
              ['vertical', t('settings.appearance.layoutSide')],
              ['horizontal', t('settings.appearance.layoutTop')]
            ]}
            onChange={(tabLayout) => update({ tabLayout })}
          />
        </Row>
        {!horizontal && (
          <Row label={t('settings.appearance.side')} hint={t('settings.appearance.sideHint')}>
            <Choice
              value={settings.sidebarSide ?? 'left'}
              options={[
                ['left', t('settings.appearance.sideLeft')],
                ['right', t('settings.appearance.sideRight')]
              ]}
              onChange={(sidebarSide) => update({ sidebarSide })}
            />
          </Row>
        )}
        <Row
          label={t('settings.general.compact')}
          hint={compactKeys ? t('settings.general.compactHint', { keys: compactKeys }) : t('settings.general.compactHintNoKeys')}
        >
          <Switch on={settings.compactMode} label={t('settings.general.compact')} onChange={(on) => update({ compactMode: on })} />
        </Row>
        <Row
          label={t('settings.general.bookmarksBar')}
          hint={barKeys ? t('settings.general.bookmarksBarHint', { keys: barKeys }) : t('settings.general.bookmarksBarHintNoKeys')}
        >
          <Switch on={settings.showBookmarksBar !== false} label={t('settings.general.bookmarksBar')} onChange={(on) => update({ showBookmarksBar: on })} />
        </Row>
      </Group>
      <h3 className="st-sub">{t('settings.appearance.groupLook')}</h3>
      <Group>
        <Row label={t('settings.appearance.theme')} hint={t('settings.appearance.themeHint')}>
          <Choice
            value={settings.theme ?? 'system'}
            options={[
              ['system', t('settings.appearance.themeSystem')],
              ['light', t('settings.appearance.themeLight')],
              ['dark', t('settings.appearance.themeDark')]
            ]}
            onChange={(theme) => update({ theme })}
          />
        </Row>
        <Row label={t('settings.appearance.tint')} hint={t('settings.appearance.tintHint')}>
          <Choice
            value={settings.tint ?? 'normal'}
            options={[
              ['subtle', t('settings.appearance.tintSubtle')],
              ['normal', t('settings.appearance.tintNormal')],
              ['strong', t('settings.appearance.tintStrong')]
            ]}
            onChange={(tint) => update({ tint })}
          />
        </Row>
        <Row label={t('settings.appearance.tabPreview')} hint={t('settings.appearance.tabPreviewHint')}>
          <Switch on={settings.tabPreview !== false} label={t('settings.appearance.tabPreview')} onChange={(on) => update({ tabPreview: on })} />
        </Row>
        {IS_MAC && (
          <Row label={t('settings.appearance.transparency')} hint={t('settings.appearance.transparencyHint')}>
            <Switch on={settings.transparency !== false} label={t('settings.appearance.transparency')} onChange={(on) => update({ transparency: on })} />
          </Row>
        )}
      </Group>
      <h3 className="st-sub">{t('settings.appearance.groupText')}</h3>
      <Group>
        <Row label={t('settings.appearance.density')} hint={t('settings.appearance.densityHint')}>
          <Choice
            value={settings.density ?? 'normal'}
            options={[
              ['compact', t('settings.appearance.densityCompact')],
              ['normal', t('settings.appearance.densityNormal')],
              ['roomy', t('settings.appearance.densityRoomy')]
            ]}
            onChange={(density) => update({ density })}
          />
        </Row>
        <Row label={t('settings.appearance.textSize')} hint={t('settings.appearance.textSizeHint')}>
          <Choice
            value={settings.textSize ?? 'normal'}
            options={[
              ['small', t('settings.appearance.textSmall')],
              ['normal', t('settings.appearance.textNormal')],
              ['large', t('settings.appearance.textLarge')]
            ]}
            onChange={(textSize) => update({ textSize })}
          />
        </Row>
      </Group>
      <h3 className="st-sub">{t('settings.appearance.groupWeb')}</h3>
      <Group>
        <Row label={t('settings.appearance.fontSize')} hint={t('settings.appearance.fontSizeHint')}>
          <select
            className="st-select"
            value={settings.fontSize ?? DEFAULT_FONT_SIZE}
            aria-label={t('settings.appearance.fontSize')}
            onChange={(e) => update({ fontSize: Number(e.target.value) })}
          >
            {FONT_SIZES.map((size, i) => (
              <option key={size} value={size}>
                {t(FONT_LABELS[i])}
              </option>
            ))}
          </select>
        </Row>
        <Row label={t('settings.appearance.minFontSize')} hint={t('settings.appearance.minFontSizeHint')}>
          <select
            className="st-select"
            value={settings.minimumFontSize ?? 0}
            aria-label={t('settings.appearance.minFontSize')}
            onChange={(e) => update({ minimumFontSize: Number(e.target.value) })}
          >
            {MIN_FONT_SIZES.map((size) => (
              <option key={size} value={size}>
                {size === 0 ? t('settings.appearance.minFontNone') : t('settings.appearance.minFontPx', { size })}
              </option>
            ))}
          </select>
        </Row>
        <Row label={t('settings.appearance.defaultZoom')} hint={t('settings.appearance.defaultZoomHint')}>
          <select
            className="st-select"
            value={settings.defaultZoom ?? DEFAULT_ZOOM}
            aria-label={t('settings.appearance.defaultZoom')}
            onChange={(e) => update({ defaultZoom: Number(e.target.value) })}
          >
            {ZOOM_PERCENTS.map((percent) => (
              <option key={percent} value={percent}>
                {t('settings.appearance.defaultZoomPercent', { percent })}
              </option>
            ))}
          </select>
        </Row>
      </Group>
      <p className="st-note">{t('settings.appearance.webNote')}</p>
      {IS_MAC && (
        <>
          <h3 className="st-sub">{t('settings.appearance.icon')}</h3>
          <div className="st-icons">
            {info.appIcons.map((icon) => (
              <button
                key={icon.id}
                className={`st-icon${icon.id === settings.appIcon ? ' on' : ''}`}
                aria-pressed={icon.id === settings.appIcon}
                title={icon.name}
                onClick={() => update({ appIcon: icon.id })}
              >
                <img src={icon.preview} alt="" width={40} height={40} draggable={false} />
                <span>{icon.name}</span>
              </button>
            ))}
          </div>
          <p className="st-note">{t('settings.appearance.note')}</p>
        </>
      )}
    </>
  )
}

const DATA_KINDS: { kind: BrowsingDataKind; label: (c: SettingsInfo['counts'], t: TFunction) => string }[] = [
  { kind: 'history', label: (c, t) => t('settings.privacy.history', { count: c.history }) },
  { kind: 'archive', label: (c, t) => t('settings.privacy.archive', { count: c.archive }) },
  { kind: 'downloads', label: (c, t) => t('settings.privacy.downloads', { count: c.downloads }) },
  { kind: 'cache', label: (_, t) => t('settings.privacy.cache') },
  { kind: 'cookies', label: (_, t) => t('settings.privacy.cookies') },
  { kind: 'permissions', label: (c, t) => t('settings.privacy.permissions', { count: c.permissions }) }
]

function Privacy({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const [picked, setPicked] = useState<Set<BrowsingDataKind>>(new Set())
  const [confirming, setConfirming] = useState<'selection' | 'all' | null>(null)
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState(false)
  const [failed, setFailed] = useState(false)
  // "Select all" three-state checkbox
  const allRef = useRef<HTMLInputElement>(null)
  const allPicked = picked.size === DATA_KINDS.length
  useEffect(() => {
    if (allRef.current) allRef.current.indeterminate = picked.size > 0 && !allPicked
  }, [picked, allPicked])

  const toggleAll = () => {
    setConfirming(null)
    setDone(false)
    setFailed(false)
    setPicked(allPicked ? new Set() : new Set(DATA_KINDS.map((d) => d.kind)))
  }
  const toggle = (kind: BrowsingDataKind) => {
    setConfirming(null)
    setDone(false)
    setFailed(false)
    setPicked((prev) => {
      const next = new Set(prev)
      if (!next.delete(kind)) next.add(kind)
      return next
    })
  }
  const clear = async () => {
    const kinds = confirming === 'all' ? DATA_KINDS.map((d) => d.kind) : [...picked]
    setBusy(true)
    setFailed(false)
    try {
      await window.signet.clearBrowsingData(kinds)
      setPicked(new Set())
      setDone(true)
    } catch {
      setFailed(true)
    } finally {
      setBusy(false)
      setConfirming(null)
    }
  }

  return (
    <>
      <Group>
        <Row label={t('settings.privacy.adblock')}>
          <Switch on={info.settings.adblock} label={t('settings.privacy.adblock')} onChange={(on) => window.signet.updateSettings({ adblock: on })} />
        </Row>
        <Row label={t('settings.privacy.httpsOnly')} hint={t('settings.privacy.httpsOnlyHint')}>
          <Switch
            on={info.settings.httpsOnly === true}
            label={t('settings.privacy.httpsOnly')}
            onChange={(on) => window.signet.updateSettings({ httpsOnly: on })}
          />
        </Row>
      </Group>
      <SecureDns info={info} />
      <h3 className="st-sub">{t('settings.privacy.clearData')}</h3>
      <Group>
        <label className="st-row st-check st-check-all">
          <input ref={allRef} type="checkbox" checked={allPicked} onChange={toggleAll} />
          <span>{t('settings.privacy.selectAll')}</span>
        </label>
        {DATA_KINDS.map(({ kind, label }) => (
          <label key={kind} className="st-row st-check">
            <input type="checkbox" checked={picked.has(kind)} onChange={() => toggle(kind)} />
            <span>{label(info.counts, t)}</span>
          </label>
        ))}
        <div className="st-row st-actions">
          {done && (
            <span className="st-ok">
              <Check size={14} strokeWidth={2.4} /> {t('settings.privacy.cleared')}
            </span>
          )}
          {failed && (
            <span className="st-err" role="alert">
              {t('settings.privacy.clearFailed')}
            </span>
          )}
          {confirming ? (
            <>
              <span className="st-hint grow">{t(confirming === 'all' ? 'settings.privacy.confirmAll' : 'settings.privacy.confirm')}</span>
              <button className="st-btn" disabled={busy} onClick={() => setConfirming(null)}>
                {t('common.cancel')}
              </button>
              <button className="st-btn danger solid" disabled={busy} onClick={clear}>
                {t('settings.privacy.clear')}
              </button>
            </>
          ) : (
            <>
              <button className="st-btn danger" disabled={busy} onClick={() => setConfirming('all')}>
                <Trash2 size={13} />
                {t('settings.privacy.clearAllEllipsis')}
              </button>
              <button className="st-btn danger" disabled={picked.size === 0 || busy} onClick={() => setConfirming('selection')}>
                <Trash2 size={13} />
                {t('settings.privacy.clearEllipsis')}
              </button>
            </>
          )}
        </div>
      </Group>
      <SitePermissionSettings />
    </>
  )
}

/** Window for choosing the data to import from a source: everything checked at the start. */
function ImportPicker({
  source,
  onCancel,
  onStart
}: {
  source: SettingsInfo['importSources'][number]
  onCancel: () => void
  onStart: (kinds: ImportKind[]) => void
}) {
  const { t } = useTranslation()
  const [picked, setPicked] = useState<Set<ImportKind>>(() => new Set(source.kinds))
  const toggle = (k: ImportKind) =>
    setPicked((prev) => {
      const next = new Set(prev)
      if (!next.delete(k)) next.add(k)
      return next
    })
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onCancel()
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onCancel])
  return (
    <div className="st-modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className="st-modal" role="dialog" aria-modal="true" aria-labelledby="st-import-title">
        <h2 id="st-import-title">{t('settings.import.choose', { name: source.title })}</h2>
        <p className="st-note">{t('settings.import.chooseHint')}</p>
        <div className="st-group">
          {source.kinds.map((k) => (
            <label key={k} className="st-row st-check">
              <input type="checkbox" checked={picked.has(k)} onChange={() => toggle(k)} />
              <span>{t(`settings.import.kind.${k}`)}</span>
              {k === 'passwords' && <span className="st-hint">{t('settings.import.passwordsHint')}</span>}
              {k === 'extensions' && <span className="st-hint">{t('settings.import.extensionsHint')}</span>}
            </label>
          ))}
        </div>
        <div className="st-modal-actions">
          <button className="st-btn" onClick={onCancel}>
            {t('settings.import.cancel')}
          </button>
          <button className="st-btn solid" disabled={picked.size === 0} autoFocus onClick={() => onStart(source.kinds.filter((k) => picked.has(k)))}>
            {t('settings.import.start')}
          </button>
        </div>
      </div>
    </div>
  )
}

function Import({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState<string | null>(null)
  const [choosing, setChoosing] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  const run = async (id: string, kinds: ImportKind[]) => {
    setChoosing(null)
    setBusy(id)
    setFailed(false)
    try {
      await window.signet.runImport(id, kinds)
    } catch {
      setFailed(true)
    } finally {
      setBusy(null)
    }
  }
  const chosen = info.importSources.find((s) => s.id === choosing)
  return (
    <>
      <Group>
        {info.importSources.map((src) => (
          <Row key={src.id} label={src.title}>
            {src.available ? (
              <button
                className="st-btn"
                data-import={src.id}
                disabled={busy !== null}
                // Only one data item (Safari): no choice to make.
                onClick={() => (src.kinds.length > 1 ? setChoosing(src.id) : void run(src.id, src.kinds))}
              >
                {busy === src.id ? t('settings.import.importing') : t('settings.import.run')}
              </button>
            ) : (
              <span className="st-hint">{t('settings.import.unavailable')}</span>
            )}
          </Row>
        ))}
      </Group>
      {failed && (
        <p className="st-note st-err" role="alert">
          {t('settings.import.failed')}
        </p>
      )}
      {IS_MAC && <p className="st-note">{t('settings.import.note')}</p>}
      {chosen && <ImportPicker source={chosen} onCancel={() => setChoosing(null)} onStart={(kinds) => void run(chosen.id, kinds)} />}
    </>
  )
}

function Boosts({ info }: { info: SettingsInfo }) {
  const { t, i18n } = useTranslation()
  const signet = window.signet
  const [confirming, setConfirming] = useState<string | null>(null)
  const boostKeys = shortcutLabel('customizeSite', info.settings.shortcuts, IS_MAC)
  if (info.boosts.length === 0) {
    return (
      <div className="st-empty">
        <Rocket size={28} strokeWidth={1.5} />
        <p>{t('settings.boosts.empty1')}</p>
        <p>
          {boostKeys ? (
            <Trans i18nKey="settings.boosts.empty2" values={{ keys: boostKeys }} components={{ kbd: <kbd /> }} />
          ) : (
            t('settings.boosts.empty2NoKeys')
          )}
        </p>
      </div>
    )
  }
  return (
    <Group>
      {info.boosts.map((boost) => {
        const hidden = boost.zapped.length
        return (
          <div key={boost.site} className="st-row">
            <div className="st-label">
              <span>{boost.site}</span>
              <span className="st-hint">
                {hidden === 0 ? t('settings.boosts.noneHidden') : t('settings.boosts.hidden', { count: hidden })} ·{' '}
                {new Date(boost.updatedAt).toLocaleDateString(i18n.language, { day: 'numeric', month: 'short', year: 'numeric' })}
              </span>
            </div>
            {confirming === boost.site ? (
              <InlineConfirm
                message={t('settings.boosts.confirm', { site: boost.site })}
                confirmLabel={t('settings.boosts.delete')}
                onCancel={() => setConfirming(null)}
                onConfirm={() => {
                  setConfirming(null)
                  void signet.deleteBoost(boost.site)
                }}
              />
            ) : (
              <>
                <Switch
                  on={boost.enabled}
                  label={t('settings.boosts.toggle', { site: boost.site })}
                  onChange={(enabled) => signet.saveBoost(boost.site, { enabled })}
                />
                <button className="st-btn" onClick={() => signet.openBoost(boost.site)}>
                  <Pencil size={13} />
                  {t('settings.boosts.edit')}
                </button>
                <button className="icon" title={t('settings.boosts.deleteBoost')} onClick={() => setConfirming(boost.site)}>
                  <Trash2 size={15} />
                </button>
              </>
            )}
          </div>
        )
      })}
    </Group>
  )
}

function About({ info }: { info: SettingsInfo }) {
  const { t } = useTranslation()
  const icon = info.appIcons.find((i) => i.id === info.settings.appIcon) ?? info.appIcons[0]
  return (
    <>
      <div className="st-about">
        {icon && <img src={icon.preview} alt="" width={96} height={96} draggable={false} />}
        <h2>Signet</h2>
        <span className="st-hint">{t('settings.about.version', { version: info.version })}</span>
        <a href={REPO} target="_blank" rel="noreferrer">
          {REPO.replace('https://', '')}
        </a>
        <button className="st-btn" onClick={() => void window.signet.checkForUpdates()}>
          {t('settings.about.checkUpdates')}
        </button>
        <button className="st-btn" onClick={() => void window.signet.openWelcome()}>
          {t('settings.about.intro')}
        </button>
      </div>
      <Group>
        <Row label={t('settings.about.autoUpdates')} hint={t('settings.about.autoUpdatesHint')}>
          <Switch
            on={info.settings.checkUpdates !== false}
            label={t('settings.about.autoUpdates')}
            onChange={(checkUpdates) => window.signet.updateSettings({ checkUpdates })}
          />
        </Row>
      </Group>
    </>
  )
}
