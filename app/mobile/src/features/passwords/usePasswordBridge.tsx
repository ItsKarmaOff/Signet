// Bridge between the page (injected script) and the vault: offer to save after a login, key button to fill.
// One instance per tab (BrowserView). Entered credentials only live in the component's memory, for the time of the answer.
import { useLocalSearchParams } from 'expo-router'
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { credentialsForPage, isNever, matchesPage, planSave, type Credential, type CredentialMeta } from '@signet/shared/mobile/passwords'
import { siteOf } from '@signet/shared/site'
import type { TabInfo } from '@signet/shared/types'
import { haptic } from '@/lib/haptics'
import { t } from '@/lib/i18n'
import { useStore } from '@/state'
import { toast } from '@/ui'
import { patchTabView } from '../browser/viewStore'
import { authenticate } from './auth'
import { PasswordOverlay, type SavePrompt } from './PasswordOverlay'
import { fillScript, PASSWORD_SCRIPT } from './script'
import { addCredential, credentialsFor, getNever, neverSaveFor, updateCredential, useVaultMetas } from './vault'

/** An unanswered offer is forgotten after 2 minutes. */
const PROMPT_TTL = 2 * 60_000
/** Username entered alone (two-step login): kept for 5 minutes for the password page. */
const IDENTIFIER_TTL = 5 * 60_000
const MAX_FIELD = 1000

interface WebHandle {
  injectJavaScript: (js: string) => void
}

const originOf = (url: string): string | null => {
  try {
    const u = new URL(url)
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.origin : null
  } catch {
    return null
  }
}

export interface PasswordBridge {
  /** To add to the injected scripts after the page loads. */
  injectedScript: string
  /** Receives the page's `pw:*` messages. */
  onMessage: (msg: unknown) => void
  /** Fill the page's form (asks for Face ID): key button and page menu. */
  fill: () => void
  /** To show in the tab's view: save offer and key button. */
  overlay: ReactNode
}

export function usePasswordBridge(webRef: { current: WebHandle | null }, tab: TabInfo, isPrivate: boolean, visible: boolean): PasswordBridge {
  const offer = useStore((s) => s.settings.offerToSavePasswords)
  const metas = useVaultMetas()
  const [hasForm, setHasForm] = useState(false)
  const [prompt, setPrompt] = useState<SavePrompt | null>(null)
  const [picker, setPicker] = useState<Credential[] | null>(null)
  const urlRef = useRef(tab.url)
  const identifier = useRef<{ origin: string; username: string; at: number } | null>(null)
  const busy = useRef(false)
  urlRef.current = tab.url

  // DEV: `/?dev=pwsave` shows the save card, `/?dev=pwchip` the key button (acceptance-test captures cannot touch the screen).
  const { dev } = useLocalSearchParams<{ dev?: string }>()
  const devChip = __DEV__ && (dev === 'pwchip' || dev === 'menupw')
  useEffect(() => {
    if (!__DEV__ || dev !== 'pwsave' || !visible) return
    const id = setTimeout(
      () => setPrompt({ origin: originOf(urlRef.current) ?? 'https://example.com', username: 'alex@example.com', password: 'hunter2-demo' }),
      1500
    )
    return () => clearTimeout(id)
  }, [dev, visible])

  // New address: the key button resets (the script restores it if the page has a form).
  useEffect(() => {
    setHasForm(false)
  }, [tab.url])

  useEffect(() => {
    if (!prompt) return
    const timer = setTimeout(() => setPrompt(null), PROMPT_TTL)
    return () => clearTimeout(timer)
  }, [prompt])

  // Tab switched to private, or setting turned off: nothing stays pending.
  useEffect(() => {
    if (isPrivate || !offer) setPrompt(null)
  }, [isPrivate, offer])

  const matches = useMemo<CredentialMeta[]>(() => credentialsForPage(metas, tab.url), [metas, tab.url])

  // The page menu offers "Fill" as soon as the key button would.
  const canFill = devChip || (hasForm && matches.length > 0)
  useEffect(() => {
    patchTabView(tab.id, { canFill })
  }, [tab.id, canFill])

  const onSubmit = useCallback(
    async (msg: Record<string, unknown>) => {
      const { settings } = useStore.getState()
      if (isPrivate || !settings.offerToSavePasswords) return
      const origin = typeof msg.origin === 'string' ? msg.origin : ''
      // The origin announced by the page must be the tab's: a page cannot make a login be saved for another site.
      if (!origin || origin !== originOf(urlRef.current)) return
      let username = typeof msg.username === 'string' ? msg.username.trim().slice(0, MAX_FIELD) : ''
      const password = typeof msg.password === 'string' ? msg.password : ''
      if (!password) {
        if (username) identifier.current = { origin, username, at: Date.now() }
        return
      }
      if (password.length > MAX_FIELD) return
      const known = identifier.current
      if (!username && known && known.origin === origin && Date.now() - known.at < IDENTIFIER_TTL) username = known.username
      identifier.current = null
      if (isNever(getNever(), origin)) return
      const existing = await credentialsFor(origin)
      const plan = planSave(existing, { origin, username, password })
      if (plan.action === 'none') return
      haptic('light')
      setPrompt({ origin, username, password, updateId: plan.action === 'update' ? plan.id : undefined })
    },
    [isPrivate]
  )

  const onMessage = useCallback(
    (raw: unknown) => {
      const msg = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>
      if (msg.type === 'pw:form') setHasForm(msg.has === true)
      else if (msg.type === 'pw:submit') void onSubmit(msg)
      else if (msg.type === 'pw:filled' && msg.ok === false && visible) toast.show({ message: t('passwords.fillFailed') })
    },
    [onSubmit, visible]
  )

  const fill = useCallback(
    (c: Credential) => {
      const origin = originOf(urlRef.current)
      if (!origin || !matchesPage(c.origin, urlRef.current)) return
      webRef.current?.injectJavaScript(fillScript(origin, c.username, c.password))
      void updateCredential(c.id, { lastUsed: Date.now() })
      haptic('success')
    },
    [webRef]
  )

  const onFill = useCallback(async () => {
    if (busy.current) return
    busy.current = true
    try {
      if (!(await authenticate(t('passwords.authFill')))) return
      const list = await credentialsFor(urlRef.current)
      if (list.length === 0) return
      if (list.length === 1) fill(list[0])
      else setPicker(list)
    } finally {
      busy.current = false
    }
  }, [fill])

  const answer = useCallback(
    async (what: 'save' | 'later' | 'never') => {
      const p = prompt
      setPrompt(null)
      if (!p || what === 'later') return
      const site = siteOf(p.origin) || p.origin
      if (what === 'never') {
        await neverSaveFor(p.origin)
        toast.show({ message: t('passwords.neverDone', { site }) })
        return
      }
      const ok = p.updateId ? await updateCredential(p.updateId, { password: p.password }) : !!(await addCredential(p))
      toast.show({ message: ok ? t(p.updateId ? 'passwords.updated' : 'passwords.saved') : t('passwords.saveFailed') })
      if (ok) haptic('success')
    },
    [prompt]
  )

  const overlay = visible ? (
    <PasswordOverlay
      prompt={prompt}
      onAnswer={(what) => void answer(what)}
      canFill={canFill}
      onFill={() => void onFill()}
      picker={picker}
      onPick={(c) => {
        setPicker(null)
        if (c) fill(c)
      }}
    />
  ) : null

  return { injectedScript: PASSWORD_SCRIPT, onMessage, overlay, fill: () => void onFill() }
}
