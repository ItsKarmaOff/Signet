// Searchable list of countries (names in the app language), shown in a sheet.
import { useMemo, useState } from 'react'
import { countryList } from '@signet/shared/autofill/values'
import { normalizeText } from '@signet/shared/autofill/classify'
import { useLocale, useT } from '@/lib/i18n'
import { EmptyState, ListRow, ListSection, ModalSheet, SearchField } from '@/ui'

export interface CountrySheetProps {
  visible: boolean
  /** Selected ISO code, or empty. */
  value: string
  onSelect: (code: string) => void
  onClose: () => void
}

export function CountrySheet({ visible, value, onSelect, onClose }: CountrySheetProps) {
  const t = useT()
  const locale = useLocale()
  const [query, setQuery] = useState('')
  const all = useMemo(() => countryList(locale), [locale])
  const q = normalizeText(query)
  const shown = q ? all.filter((c) => normalizeText(c.name).includes(q)) : all
  const pick = (code: string) => {
    setQuery('')
    onSelect(code)
  }

  return (
    <ModalSheet
      visible={visible}
      title={t('autofill.country')}
      cancelLabel={t('common.cancel')}
      onClose={() => {
        setQuery('')
        onClose()
      }}
    >
      <SearchField
        value={query}
        onChangeText={setQuery}
        clearLabel={t('browser.clear')}
        placeholder={t('autofill.searchCountries')}
        accessibilityLabel={t('autofill.searchCountries')}
        autoCorrect={false}
      />
      <ListSection>
        {!q ? <ListRow title={t('autofill.countryNone')} selected={!value} onPress={() => pick('')} /> : null}
        {shown.map((c) => (
          <ListRow key={c.code} title={c.name} selected={c.code === value} onPress={() => pick(c.code)} />
        ))}
      </ListSection>
      {q && shown.length === 0 ? <EmptyState title={t('autofill.noCountryResults')} /> : null}
    </ModalSheet>
  )
}
