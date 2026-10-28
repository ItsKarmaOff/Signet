import {
  EllipsisVertical,
  ArrowLeft,
  ArrowRight,
  BookOpen,
  CircleX,
  Code,
  FlaskConical,
  Info,
  TriangleAlert,
  KeyRound,
  Languages,
  Lock,
  PanelLeft,
  PanelRight,
  RotateCw,
  Rocket,
  Smartphone,
  Star,
  X
} from 'lucide-react'
import { memo } from 'react'
import type { MouseEvent } from 'react'
import { useTranslation } from 'react-i18next'
import type { Bookmark, Snapshot } from '@shared/types'
import { ExtensionButtons } from './ExtensionButtons'
import { barStyle, domainOf, isSecure, languageName, siteOf } from '../lib/util'
import { IS_MAC } from '@signet/shared/i18n'
import { shortcutLabel } from '@shared/shortcuts'
import { aliasFor, isLocalDevUrl } from '@shared/devhosts'
import { deviceSpec } from '@shared/devices'
import { useLinkState } from '../lib/useLinkState'

const norm = (u: string) => u.replace(/\/$/, '')
const EMPTY: Bookmark[] = []
const urlSets = new WeakMap<Bookmark[], Set<string>>()
/** Normalized URLs of all bookmarks (folders included), computed once per list. */
function urlsOf(items: Bookmark[]): Set<string> {
  let set = urlSets.get(items)
  if (!set) {
    set = new Set()
    const walk = (list: Bookmark[]) => {
      for (const i of list) {
        if (i.kind === 'link') set!.add(norm(i.url))
        else walk(i.items)
      }
    }
    walk(items)
    urlSets.set(items, set)
  }
  return set
}

interface Props {
  s: Snapshot
  /** Tab targeted by the bar; the active tab by default. */
  tabId?: string
  /** Reserves room for the traffic lights (card at the very top left, sidebar hidden). */
  leading?: boolean
  /** Shows the sidebar toggle (only one bar at a time, the top-left pane's). */
  sidebarToggle?: boolean
  /** Narrow card: only Reload, padlock and domain. */
  narrow?: boolean
  /** Pane without focus (split view): dimmed content. */
  dim?: boolean
  /** Floating bar of compact mode (fills its view). */
  floating?: boolean
}

/** What the bar reads from the Snapshot: used to redraw it only if one of these values changed. */
function deps(s: Snapshot, tabId?: string): unknown[] {
  const space = s.spaces.find((x) => x.id === s.activeSpaceId) ?? s.spaces[0]
  const id = tabId ?? space.activeTabId ?? undefined
  return [
    space.id,
    space.profile,
    space.activeTabId,
    id,
    id ? s.tabs[id] : undefined,
    id ? s.favorites.includes(id) : false,
    s.boostedSites,
    s.settings.extensionOrder,
    s.settings.pinnedExtensions,
    s.settings.shortcuts,
    s.settings.devMode,
    s.settings.devAliases,
    s.sidebarVisible,
    s.settings.sidebarSide,
    space.bookmarks
  ]
}

function sameToolbar(a: Props, b: Props): boolean {
  if (
    a.tabId !== b.tabId ||
    a.leading !== b.leading ||
    a.sidebarToggle !== b.sidebarToggle ||
    a.narrow !== b.narrow ||
    a.dim !== b.dim ||
    a.floating !== b.floating
  )
    return false
  if (a.s === b.s) return true
  const x = deps(a.s, a.tabId)
  const y = deps(b.s, b.tabId)
  return x.every((v, i) => v === y[i])
}

/** A pane's bar: blended into the site (color of the top of the page), actions on the pane's tab. */
export const Toolbar = memo(function Toolbar({ s, tabId, leading = false, sidebarToggle = false, narrow = false, dim = false, floating = false }: Props) {
  const { t, i18n } = useTranslation()
  const signet = window.signet
  const phones = useLinkState()?.active ?? 0
  const space = s.spaces.find((x) => x.id === s.activeSpaceId) ?? s.spaces[0]
  const id = tabId ?? space.activeTabId ?? undefined
  const tab = id ? s.tabs[id] : undefined
  const url = tab?.url ?? ''
  // Current shortcuts (editable in Settings) for the tooltips.
  const sidebarKeys = shortcutLabel('toggleSidebar', s.settings.shortcuts, IS_MAC)
  const readerKeys = shortcutLabel('reader', s.settings.shortcuts, IS_MAC)
  const bookmarkKeys = shortcutLabel('bookmark', s.settings.shortcuts, IS_MAC)
  // Local name (Settings › Development): shown in place of the host and port.
  const alias = aliasFor(url, s.settings.devAliases)
  const domain = alias ? alias.name : url ? domainOf(url) : ''
  const title = tab?.title && tab.title !== domain ? tab.title : ''
  const site = /^https?:/i.test(url) ? siteOf(url) : ''
  const boosted = !!site && s.boostedSites.includes(site)
  // Developer mode, local page: the port next to the host, and the console counter.
  const local = s.settings.devMode === true && isLocalDevUrl(url)
  const port = local && !alias ? new URL(url).port : ''
  const errors = local ? (tab?.consoleErrors ?? 0) : 0
  const warnings = local ? (tab?.consoleWarnings ?? 0) : 0
  const count = (n: number) => (n > 99 ? '99+' : String(n))
  // Development settings active on the tab (Development menu): shown as a reminder while they last.
  const ov = tab?.devOverrides
  const overrides = ov
    ? [
        ov.noCache && t('develop.noCache'),
        ov.noJs && t('develop.noJs'),
        ov.scheme && t(ov.scheme === 'dark' ? 'develop.schemeDark' : 'develop.schemeLight'),
        ov.reducedMotion && t('develop.reducedMotion'),
        ov.network && t(`develop.network.${ov.network}`),
        ov.userAgent && t(`develop.ua.${ov.userAgent}`),
        ov.responsive &&
          t('develop.responsive', {
            device: deviceSpec(ov.responsive.device).name ?? t(`develop.device.${ov.responsive.device as 'laptop' | 'desktop' | 'custom'}`)
          })
      ].filter((x): x is string => !!x)
    : []
  const bookmarkable = /^(https?|file|signet):/i.test(url)
  const bookmarked = bookmarkable && urlsOf(space.bookmarks ?? EMPTY).has(norm(url))
  // Extensions of an isolated space live in its partition; favorites and shared spaces, in the default session.
  const partition = space.profile === 'isolated' && !(id && s.favorites.includes(id)) ? `persist:space-${space.id}` : '_self'

  /** Main's windows (site panel, address) target the active tab: focus it first. */
  const focusTab = () => {
    if (id && id !== space.activeTabId) void signet.activateTab(id)
  }
  const openPanel = (e: MouseEvent<HTMLButtonElement>) => {
    const r = e.currentTarget.getBoundingClientRect()
    focusTab()
    void signet.openSitePanel({ x: r.left, y: r.top, width: r.width, height: r.height })
  }

  return (
    <div
      className={`toolbar${leading ? ' leading' : ''}${floating ? ' floating' : ''}${dim ? ' dim' : ''}`}
      style={floating ? barStyle(tab?.themeColor) : undefined}
    >
      {sidebarToggle && (
        <button
          className="icon"
          title={`${s.sidebarVisible ? t('toolbar.hideSidebar') : t('toolbar.showSidebar')}${sidebarKeys ? ` (${sidebarKeys})` : ''}`}
          onClick={() => signet.toggleSidebar()}
        >
          {s.settings.sidebarSide === 'right' ? <PanelRight size={17} /> : <PanelLeft size={17} />}
        </button>
      )}
      {!narrow && (
        <>
          <button className="icon" title={t('common.back')} disabled={!tab?.canGoBack} onClick={() => signet.goBack(id)}>
            <ArrowLeft size={17} />
          </button>
          <button className="icon" title={t('common.forward')} disabled={!tab?.canGoForward} onClick={() => signet.goForward(id)}>
            <ArrowRight size={17} />
          </button>
        </>
      )}
      <button className="icon" title={tab?.loading ? t('toolbar.stop') : t('toolbar.reload')} disabled={!tab} onClick={() => signet.reload(id)}>
        {tab?.loading ? <X size={16} /> : <RotateCw size={15} />}
      </button>
      <div className="address-line">
        {site && (
          <button className="icon site-btn" title={isSecure(url) ? t('toolbar.secure') : t('toolbar.insecure')} onClick={openPanel}>
            {isSecure(url) ? <Lock size={14} /> : <Info size={15} />}
          </button>
        )}
        <button
          className="address"
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect()
            focusTab()
            // The address field serves as the anchor: the command bar grows from it.
            void signet.openPalette('edit-url', { x: r.left, y: r.top, width: r.width, height: r.height })
          }}
        >
          {url ? (
            <>
              <span className="domain" title={alias ? url : undefined}>
                {domain}
                {port && <span className="port">:{port}</span>}
              </span>
              {title && !narrow && <span className="page">/ {title}</span>}
            </>
          ) : (
            <span className="page">{t('toolbar.addressPlaceholder')}</span>
          )}
        </button>
        {(errors > 0 || warnings > 0) && (
          <button
            className="console-badge"
            title={[
              errors ? t('toolbar.consoleErrors', { count: errors }) : '',
              warnings ? t('toolbar.consoleWarnings', { count: warnings }) : '',
              t('toolbar.openConsole')
            ]
              .filter(Boolean)
              .join(' · ')}
            onClick={() => {
              focusTab()
              if (id) void signet.showConsole(id)
            }}
          >
            {errors > 0 && (
              <span className="err">
                <CircleX size={12} strokeWidth={2.4} />
                {count(errors)}
              </span>
            )}
            {warnings > 0 && (
              <span className="warn">
                <TriangleAlert size={12} strokeWidth={2.4} />
                {count(warnings)}
              </span>
            )}
          </button>
        )}
        {!narrow && (tab?.foreignLang || tab?.translation) && (
          <button
            className={`icon translate-btn${tab.translation === 'on' ? ' on' : tab.translation === 'working' ? ' working' : ''}`}
            title={tab.translation ? t('toolbar.showOriginal') : t('toolbar.translate', { language: languageName(tab.foreignLang ?? '', i18n.language) })}
            aria-pressed={!!tab.translation}
            onClick={() => {
              focusTab()
              if (id) void signet.toggleTranslation(id)
            }}
          >
            <Languages size={15} />
          </button>
        )}
        {!narrow && tab?.reader && (
          <button
            className={`icon reader-btn${tab.reader === 'on' ? ' on' : ''}`}
            title={tab.reader === 'on' ? t('toolbar.exitReader') : readerKeys ? t('toolbar.reader', { keys: readerKeys }) : t('toolbar.readerNoKeys')}
            aria-pressed={tab.reader === 'on'}
            onClick={() => {
              focusTab()
              if (id) void signet.toggleReader(id)
            }}
          >
            <BookOpen size={15} />
          </button>
        )}
        {!narrow && tab?.passwordKey && (
          <button
            className={`icon key-btn${tab.passwordKey === 'pending' ? ' on' : ''}`}
            title={tab.passwordKey === 'pending' ? t('toolbar.savePassword') : t('toolbar.sitePasswords')}
            onClick={(e) => {
              const r = e.currentTarget.getBoundingClientRect()
              focusTab()
              if (id) void signet.openPasswordKey(id, { x: r.left, y: r.top, width: r.width, height: r.height })
            }}
          >
            <KeyRound size={15} />
          </button>
        )}
        {!narrow && bookmarkable && (
          <button
            className={`icon star-btn${bookmarked ? ' on' : ''}`}
            title={
              bookmarkKeys
                ? t(bookmarked ? 'toolbar.removeBookmark' : 'toolbar.addBookmark', { keys: bookmarkKeys })
                : t(bookmarked ? 'toolbar.removeBookmarkNoKeys' : 'toolbar.addBookmarkNoKeys')
            }
            onClick={() => signet.toggleBookmark(id ?? null)}
          >
            <Star size={15} fill={bookmarked ? 'currentColor' : 'none'} />
          </button>
        )}
      </div>
      {!narrow && (
        <ExtensionButtons
          partition={partition}
          order={s.settings.extensionOrder ?? []}
          pinned={s.settings.pinnedExtensions ?? []}
          focusTab={focusTab}
          onOrder={(extensionOrder) => signet.updateSettings({ extensionOrder })}
          onMenu={(extId) => signet.showExtensionMenu(extId)}
        />
      )}
      {overrides.length > 0 && (
        <button
          className="icon develop-btn on"
          title={t('toolbar.devOverrides', { list: overrides.join(', ') })}
          onClick={() => {
            focusTab()
            if (id) void signet.showDevelopMenu(id)
          }}
        >
          <FlaskConical size={16} />
        </button>
      )}
      {!narrow && tab?.devtools && (
        <button
          className="icon devtools-btn on"
          title={t('toolbar.devtools')}
          onClick={() => {
            focusTab()
            if (id) void signet.showDevToolsMenu(id)
          }}
        >
          <Code size={16} />
        </button>
      )}
      {!narrow && phones > 0 && (
        <button className="icon phone-btn" title={t('toolbar.phoneConnected', { count: phones })} onClick={() => void signet.openSettings('devices')}>
          <Smartphone size={15} />
        </button>
      )}
      {!narrow && (
        <button
          className={`icon boost-btn${boosted ? ' on' : ''}`}
          title={boosted ? t('toolbar.boostOn') : t('toolbar.boostSite')}
          disabled={!site}
          onClick={() => signet.openBoost(site)}
        >
          <Rocket size={16} />
        </button>
      )}
      {/* Linux and Windows: the menu bar is hidden (Alt shows it); this button opens the whole app menu. */}
      {!IS_MAC && (
        <button className="icon app-menu-btn" title={t('toolbar.appMenu')} aria-label={t('toolbar.appMenu')} onClick={() => void signet.showAppMenu()}>
          <EllipsisVertical size={16} />
        </button>
      )}
    </div>
  )
}, sameToolbar)
