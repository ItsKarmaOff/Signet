// Typed translation keys: TypeScript flags any unknown key.
import 'i18next'
import type enMain from '@signet/shared/locales/en/main.json'
import type enUi from '@signet/shared/locales/en/ui.json'

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'ui'
    resources: { main: typeof enMain; ui: typeof enUi }
  }
}
