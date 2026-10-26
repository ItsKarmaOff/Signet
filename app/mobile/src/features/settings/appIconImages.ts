// Images of the app's 8 icons (the same as on the device home screen): icon choice, welcome page, About.
import type { AppIconId } from '@signet/shared/mobile/settings'

/* eslint-disable @typescript-eslint/no-require-imports -- static resources resolved by Metro */
export const APP_ICON_IMAGES: Record<AppIconId, number> = {
  aurore: require('../../../assets/app-icons/aurore.png'),
  ocean: require('../../../assets/app-icons/ocean.png'),
  corail: require('../../../assets/app-icons/corail.png'),
  menthe: require('../../../assets/app-icons/menthe.png'),
  nuit: require('../../../assets/app-icons/nuit.png'),
  papier: require('../../../assets/app-icons/papier.png'),
  graphite: require('../../../assets/app-icons/graphite.png'),
  or: require('../../../assets/app-icons/or.png')
}
/* eslint-enable @typescript-eslint/no-require-imports */
