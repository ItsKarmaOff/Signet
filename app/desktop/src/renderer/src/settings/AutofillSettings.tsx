import { useCallback, useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { CreditCard, Eye, EyeOff, MapPin, Pencil, Plus, Trash2 } from 'lucide-react'
import { InlineConfirm } from '../ui/InlineConfirm'
import { groupCardNumber, NETWORK_NAMES } from '@shared/autofill/card'
import { fullName } from '@shared/autofill/values'
import type { AddressInput, AutofillAddress, AutofillCard, AutofillInfo, SettingsInfo } from '@shared/types'

const MASK = '•••• •••• •••• '

/** Expiry months: "01 (January)" in the UI language. */
function monthOptions(language: string): { value: string; label: string }[] {
  let name: (m: number) => string = () => ''
  try {
    const format = new Intl.DateTimeFormat(language, { month: 'long', timeZone: 'UTC' })
    name = (m) => format.format(new Date(Date.UTC(2000, m - 1, 1)))
  } catch {
    // Unknown locale: numbers only.
  }
  return Array.from({ length: 12 }, (_, i) => {
    const n = name(i + 1)
    return { value: String(i + 1), label: n ? `${String(i + 1).padStart(2, '0')} (${n})` : String(i + 1).padStart(2, '0') }
  })
}

/** Expiry years: this year and the next 15, plus the card's own year when it is outside that range (an expired card). */
function yearOptions(current: string): string[] {
  const first = new Date().getFullYear()
  const years = Array.from({ length: 16 }, (_, i) => String(first + i))
  return /^\d{4}$/.test(current) && !years.includes(current) ? [current, ...years] : years
}
const hostOf = (origin: string) => origin.replace(/^\w+:\/\//, '')
const EMPTY_ADDRESS: AddressInput = {
  givenName: '',
  familyName: '',
  organization: '',
  street: '',
  city: '',
  postalCode: '',
  region: '',
  country: '',
  phone: '',
  email: ''
}
const dateOf = (c: AutofillCard) => `${String(c.expMonth).padStart(2, '0')}/${c.expYear}`

interface AddressDraft extends AddressInput {
  id: string | null
}

interface CardDraft {
  id: string | null
  holder: string
  number: string
  expMonth: string
  expYear: string
  nickname: string
}

/** Settings > Autofill: the two switches, saved addresses and cards, and the sites where saving is never offered. */
export function AutofillSettings({ info }: { info: SettingsInfo }) {
  const { t, i18n } = useTranslation()
  const signet = window.signet
  const { settings } = info
  const [data, setData] = useState<AutofillInfo | null>(null)
  const [loadFailed, setLoadFailed] = useState(false)
  const [addressDraft, setAddressDraft] = useState<AddressDraft | null>(null)
  const [cardDraft, setCardDraft] = useState<CardDraft | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [confirming, setConfirming] = useState<string | null>(null)
  const [shown, setShown] = useState<Record<string, string>>({})

  // Does not hide revealed numbers: the window gets focus back right after the Touch ID prompt, which reloads the list.
  // Only numbers of cards that are gone are dropped.
  const load = useCallback(() => {
    signet
      .listAutofill()
      .then((d) => {
        setData(d)
        setLoadFailed(false)
        const ids = new Set(d.cards.map((c) => c.id))
        setShown((s) => (Object.keys(s).every((id) => ids.has(id)) ? s : Object.fromEntries(Object.entries(s).filter(([id]) => ids.has(id)))))
      })
      .catch(() => setLoadFailed(true))
  }, [signet])
  useEffect(() => {
    load()
    window.addEventListener('focus', load)
    return () => window.removeEventListener('focus', load)
  }, [load])
  // A revealed number is hidden after 30 s or when the window loses focus.
  useEffect(() => {
    if (!Object.keys(shown).length) return
    const timer = setTimeout(() => setShown({}), 30_000)
    const hide = () => setShown({})
    window.addEventListener('blur', hide)
    return () => {
      clearTimeout(timer)
      window.removeEventListener('blur', hide)
    }
  }, [shown])

  const addresses = data?.addresses ?? []
  const cards = data?.cards ?? []
  const never = settings.neverSaveAutofill ?? []

  const submitAddress = async (e: FormEvent) => {
    e.preventDefault()
    if (!addressDraft) return
    const { id, ...address } = addressDraft
    const err = await signet.saveAddress(id, address)
    if (err) return setError(err)
    setAddressDraft(null)
    setError(null)
    load()
  }
  const submitCard = async (e: FormEvent) => {
    e.preventDefault()
    if (!cardDraft) return
    const err = await signet.saveCard(cardDraft.id, {
      holder: cardDraft.holder,
      number: cardDraft.number.trim() || null,
      expMonth: Number(cardDraft.expMonth),
      expYear: Number(cardDraft.expYear),
      nickname: cardDraft.nickname
    })
    if (err) return setError(err)
    // The shown number may be the old one.
    const edited = cardDraft.id
    if (edited) setShown(({ [edited]: _old, ...rest }) => rest)
    setCardDraft(null)
    setError(null)
    load()
  }
  const reveal = async (c: AutofillCard) => {
    if (shown[c.id] !== undefined) {
      setShown(({ [c.id]: _hidden, ...rest }) => rest)
      return
    }
    const number = await signet.revealCard(c.id)
    if (number !== null) setShown((s) => ({ ...s, [c.id]: groupCardNumber(number, c.network) }))
  }
  const escape = (close: () => void) => (e: React.KeyboardEvent) => {
    if (e.key !== 'Escape') return
    e.preventDefault()
    e.stopPropagation()
    close()
  }
  const addressField = (key: keyof AddressInput, label: string, extra?: { type?: string; autoFocus?: boolean }) =>
    addressDraft && (
      <input
        className="st-input"
        value={addressDraft[key]}
        type={extra?.type}
        placeholder={label}
        aria-label={label}
        spellCheck={false}
        autoComplete="off"
        autoFocus={extra?.autoFocus}
        onChange={(e) => setAddressDraft({ ...addressDraft, [key]: e.target.value })}
      />
    )
  const addressName = (a: AutofillAddress) => fullName(a) || t('settings.autofill.noName')
  const cardName = (c: AutofillCard) =>
    t('settings.autofill.cardLabel', { network: c.nickname || NETWORK_NAMES[c.network] || t('settings.autofill.cards'), last4: c.last4 })
  const switchRow = (label: string, hint: string, on: boolean, patch: (v: boolean) => void) => (
    <div className="st-row">
      <div className="st-label">
        <span>{label}</span>
        <span className="st-hint">{hint}</span>
      </div>
      <button className={`switch ${on ? 'on' : ''}`} role="switch" aria-checked={on} aria-label={label} onClick={() => patch(!on)}>
        <span className="knob" />
      </button>
    </div>
  )

  return (
    <>
      <div className="st-group">
        {switchRow(
          t('settings.autofill.switchAddresses'),
          t('settings.autofill.switchAddressesHint'),
          settings.autofillAddresses !== false,
          (v) => void signet.updateSettings({ autofillAddresses: v })
        )}
        {switchRow(
          t('settings.autofill.switchCards'),
          t('settings.autofill.switchCardsHint'),
          settings.autofillCards !== false,
          (v) => void signet.updateSettings({ autofillCards: v })
        )}
      </div>

      {loadFailed && !data && (
        <p className="st-note st-err" role="alert">
          {t('settings.autofill.loadFailed')}{' '}
          <button className="st-btn" onClick={load}>
            {t('common.retry')}
          </button>
        </p>
      )}

      <h3 className="st-sub">{t('settings.autofill.addresses')}</h3>
      <div className="st-group">
        <div className="st-row">
          <span className="grow" />
          <button
            className="st-btn"
            onClick={() => {
              setError(null)
              setCardDraft(null)
              setAddressDraft({ id: null, ...EMPTY_ADDRESS })
            }}
          >
            <Plus size={13} />
            {t('settings.autofill.addAddress')}
          </button>
        </div>
      </div>
      {addressDraft && (
        <form className="st-group st-add" onSubmit={submitAddress} onKeyDown={escape(() => (setAddressDraft(null), setError(null)))}>
          <div className="st-row">
            {addressField('givenName', t('settings.autofill.givenName'), { autoFocus: true })}
            {addressField('familyName', t('settings.autofill.familyName'))}
          </div>
          <div className="st-row">{addressField('organization', t('settings.autofill.organization'))}</div>
          <div className="st-row">
            <textarea
              className="st-input st-area"
              value={addressDraft.street}
              placeholder={t('settings.autofill.street')}
              aria-label={t('settings.autofill.street')}
              spellCheck={false}
              onChange={(e) => setAddressDraft({ ...addressDraft, street: e.target.value })}
            />
          </div>
          <div className="st-row">
            {addressField('postalCode', t('settings.autofill.postalCode'))}
            {addressField('city', t('settings.autofill.city'))}
          </div>
          <div className="st-row">
            {addressField('region', t('settings.autofill.region'))}
            {addressField('country', t('settings.autofill.country'))}
          </div>
          <div className="st-row">
            {addressField('phone', t('settings.autofill.phone'), { type: 'tel' })}
            {addressField('email', t('settings.autofill.email'), { type: 'email' })}
          </div>
          <div className="st-row st-actions">
            {error && (
              <span className="st-err grow" role="alert">
                {error}
              </span>
            )}
            <button type="button" className="st-btn" onClick={() => (setAddressDraft(null), setError(null))}>
              {t('common.cancel')}
            </button>
            <button type="submit" className="st-btn">
              {t('settings.autofill.save')}
            </button>
          </div>
        </form>
      )}
      {addresses.length === 0 && !addressDraft ? (
        <div className="st-empty">
          <MapPin size={28} strokeWidth={1.5} />
          <p>{t('settings.autofill.emptyAddresses')}</p>
        </div>
      ) : (
        <div className="st-group">
          {addresses.map((a) => (
            <div key={a.id} className="st-row">
              <div className="st-label">
                <span>{addressName(a)}</span>
                <span className="st-hint">{[a.street.split('\n')[0], a.postalCode, a.city].filter(Boolean).join(', ')}</span>
              </div>
              {confirming === a.id ? (
                <InlineConfirm
                  message={t('settings.autofill.confirmDeleteAddress', { name: addressName(a) })}
                  confirmLabel={t('settings.autofill.delete')}
                  onCancel={() => setConfirming(null)}
                  onConfirm={() => {
                    setConfirming(null)
                    void signet.deleteAddress(a.id).then(load)
                  }}
                />
              ) : (
                <>
                  <button
                    className="icon"
                    title={t('settings.autofill.edit')}
                    aria-label={t('settings.autofill.editAddressFor', { name: addressName(a) })}
                    onClick={() => {
                      setError(null)
                      setCardDraft(null)
                      const { id, givenName, familyName, organization, street, city, postalCode, region, country, phone, email } = a
                      setAddressDraft({ id, givenName, familyName, organization, street, city, postalCode, region, country, phone, email })
                    }}
                  >
                    <Pencil size={15} />
                  </button>
                  <button
                    className="icon"
                    title={t('settings.autofill.delete')}
                    aria-label={t('settings.autofill.deleteAddressFor', { name: addressName(a) })}
                    onClick={() => setConfirming(a.id)}
                  >
                    <Trash2 size={15} />
                  </button>
                </>
              )}
            </div>
          ))}
        </div>
      )}

      <h3 className="st-sub">{t('settings.autofill.cards')}</h3>
      {data && !data.encryption && (
        <p className="st-note st-err" role="alert">
          {t('settings.autofill.noEncryption')}
        </p>
      )}
      <div className="st-group">
        <div className="st-row">
          <span className="grow" />
          <button
            className="st-btn"
            disabled={!data?.encryption}
            onClick={() => {
              setError(null)
              setAddressDraft(null)
              setCardDraft({ id: null, holder: '', number: '', expMonth: '', expYear: '', nickname: '' })
            }}
          >
            <Plus size={13} />
            {t('settings.autofill.addCard')}
          </button>
        </div>
      </div>
      {cardDraft && (
        <form className="st-group st-add" onSubmit={submitCard} onKeyDown={escape(() => (setCardDraft(null), setError(null)))}>
          <div className="st-row">
            <input
              className="st-input"
              value={cardDraft.holder}
              placeholder={t('settings.autofill.holder')}
              aria-label={t('settings.autofill.holder')}
              autoComplete="off"
              autoFocus
              onChange={(e) => setCardDraft({ ...cardDraft, holder: e.target.value })}
            />
            <input
              className="st-input"
              value={cardDraft.number}
              placeholder={cardDraft.id ? t('settings.autofill.numberUnchanged') : t('settings.autofill.number')}
              aria-label={t('settings.autofill.number')}
              inputMode="numeric"
              autoComplete="off"
              spellCheck={false}
              onChange={(e) => setCardDraft({ ...cardDraft, number: e.target.value })}
            />
          </div>
          <div className="st-row">
            <select
              className="st-select"
              value={cardDraft.expMonth ? String(Number(cardDraft.expMonth)) : ''}
              aria-label={t('settings.autofill.expMonth')}
              onChange={(e) => setCardDraft({ ...cardDraft, expMonth: e.target.value })}
            >
              <option value="" disabled>
                {t('settings.autofill.expMonth')}
              </option>
              {monthOptions(i18n.language).map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
            <select
              className="st-select"
              value={cardDraft.expYear}
              aria-label={t('settings.autofill.expYear')}
              onChange={(e) => setCardDraft({ ...cardDraft, expYear: e.target.value })}
            >
              <option value="" disabled>
                {t('settings.autofill.expYear')}
              </option>
              {yearOptions(cardDraft.expYear).map((y) => (
                <option key={y} value={y}>
                  {y}
                </option>
              ))}
            </select>
            <input
              className="st-input"
              value={cardDraft.nickname}
              placeholder={t('settings.autofill.nickname')}
              aria-label={t('settings.autofill.nickname')}
              maxLength={40}
              autoComplete="off"
              onChange={(e) => setCardDraft({ ...cardDraft, nickname: e.target.value })}
            />
          </div>
          <div className="st-row st-actions">
            {error && (
              <span className="st-err grow" role="alert">
                {error}
              </span>
            )}
            <button type="button" className="st-btn" onClick={() => (setCardDraft(null), setError(null))}>
              {t('common.cancel')}
            </button>
            <button type="submit" className="st-btn">
              {t('settings.autofill.save')}
            </button>
          </div>
        </form>
      )}
      {cards.length === 0 && !cardDraft ? (
        <div className="st-empty">
          <CreditCard size={28} strokeWidth={1.5} />
          <p>{t('settings.autofill.emptyCards')}</p>
        </div>
      ) : (
        <div className="st-group">
          {cards.map((c) => (
            <div key={c.id} className="st-row">
              <div className="st-label">
                <span>{cardName(c)}</span>
                <span className="st-hint">{[c.holder, t('settings.autofill.expires', { date: dateOf(c) })].filter(Boolean).join(' · ')}</span>
              </div>
              {confirming === c.id ? (
                <InlineConfirm
                  message={t('settings.autofill.confirmDeleteCard', { last4: c.last4 })}
                  confirmLabel={t('settings.autofill.delete')}
                  onCancel={() => setConfirming(null)}
                  onConfirm={() => {
                    setConfirming(null)
                    void signet.deleteCard(c.id).then(load)
                  }}
                />
              ) : (
                <>
                  <code className="st-pw">{shown[c.id] ?? `${MASK}${c.last4}`}</code>
                  <button
                    className="icon"
                    title={shown[c.id] !== undefined ? t('settings.autofill.hide') : t('settings.autofill.show')}
                    aria-label={t(shown[c.id] !== undefined ? 'settings.autofill.hideFor' : 'settings.autofill.showFor', { last4: c.last4 })}
                    onClick={() => void reveal(c)}
                  >
                    {shown[c.id] !== undefined ? <EyeOff size={15} /> : <Eye size={15} />}
                  </button>
                  <button
                    className="icon"
                    title={t('settings.autofill.edit')}
                    aria-label={t('settings.autofill.editCardFor', { last4: c.last4 })}
                    onClick={() => {
                      setError(null)
                      setAddressDraft(null)
                      setCardDraft({ id: c.id, holder: c.holder, number: '', expMonth: String(c.expMonth), expYear: String(c.expYear), nickname: c.nickname })
                    }}
                  >
                    <Pencil size={15} />
                  </button>
                  <button
                    className="icon"
                    title={t('settings.autofill.delete')}
                    aria-label={t('settings.autofill.deleteCardFor', { last4: c.last4 })}
                    onClick={() => setConfirming(c.id)}
                  >
                    <Trash2 size={15} />
                  </button>
                </>
              )}
            </div>
          ))}
        </div>
      )}
      <p className="st-note">{t('settings.autofill.authNote')}</p>

      {never.length > 0 && (
        <>
          <h3 className="st-sub">{t('settings.autofill.never')}</h3>
          <p className="st-note">{t('settings.autofill.neverHint')}</p>
          <div className="st-group">
            {never.map((origin) => (
              <div key={origin} className="st-row">
                <div className="st-label">
                  <span>{hostOf(origin)}</span>
                </div>
                <button
                  className="icon"
                  title={t('settings.autofill.neverRemove')}
                  aria-label={t('settings.autofill.neverRemoveFor', { site: hostOf(origin) })}
                  onClick={() => signet.updateSettings({ neverSaveAutofill: never.filter((o) => o !== origin) })}
                >
                  <Trash2 size={15} />
                </button>
              </div>
            ))}
          </div>
        </>
      )}
    </>
  )
}
