// "Settings" feature: general, appearance, language, privacy and security, clear data, bookmarks import / export, about. Routes: app/settings/*. Logic: @signet/shared/mobile/settings.
// `registerThumbnailCleaner`: the tabs screen registers with it so that "Clear data" can empty the previews.
export { SettingsScreen } from './SettingsScreen'
export { SearchEngineScreen } from './SearchEngineScreen'
export { HomepageScreen } from './HomepageScreen'
export { AppIconScreen } from './AppIconScreen'
export { APP_ICON_IMAGES } from './appIconImages'
export { useSyncAppIcon } from './useSyncAppIcon'
export { DataScreen } from './DataScreen'
export { AboutScreen } from './AboutScreen'
export { TransferScreen } from './TransferScreen'
export { registerThumbnailCleaner } from './dataCleaners'
