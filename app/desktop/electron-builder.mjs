// electron-builder configuration (passed with `--config electron-builder.mjs` by the `dist*` scripts and the Build workflow: electron-builder does not find it on its own).
// App identity: `productName` and `appId` must not change (profile folder, keychain, macOS bundle identifier).
import { createRequire } from 'node:module'

// With npm workspaces, Electron is installed at the repository root: electron-builder does not find it in app/desktop/node_modules
// and would require an exact version in package.json. So we give it the version actually installed.
const electronVersion = createRequire(import.meta.url)('electron/package.json').version

export default {
  electronVersion,
  appId: 'dev.signet.app',
  afterPack: 'scripts/after-pack.mjs',
  // Update feed: the GitHub releases (electron-updater, see src/main/features/updates.ts). Files are uploaded by the
  // Build workflow, never by electron-builder (`--publish never`); this only writes app-update.yml and latest*.yml.
  publish: { provider: 'github', owner: 'ItsKarmaOff', repo: 'Signet', releaseType: 'release' },
  productName: 'Signet',
  electronLanguages: ['en', 'en-US', 'en-GB', 'fr'],
  directories: {
    output: 'dist'
  },
  files: ['out/**/*', '!out/main/dev-*.js', '!**/node_modules/**/*.map', '!**/node_modules/**/*.tsbuildinfo'],
  mac: {
    category: 'public.app-category.productivity',
    // DMG to install by hand; ZIP for the updates (electron-updater reads latest-mac.yml, which lists the ZIPs).
    target: ['dmg', 'zip'],
    // Ad-hoc signature (no Apple certificate): without any signature, macOS on Apple Silicon reports a downloaded app
    // as "damaged". With it, the user can allow it once in System Settings › Privacy & Security › Open Anyway.
    identity: '-',
    // The hardened runtime is only needed for Apple notarization; with an ad-hoc signature it blocks Electron's libraries.
    hardenedRuntime: false,
    extendInfo: {
      NSCameraUsageDescription: 'Websites can ask to use the camera.',
      NSMicrophoneUsageDescription: 'Websites can ask to use the microphone.'
    },
    icon: 'build/icon.icns',
    // One DMG per processor: Signet-X.Y.Z-arm64.dmg (Apple Silicon) and Signet-X.Y.Z-x64.dmg (Intel).
    artifactName: '${productName}-${version}-${arch}.${ext}'
  },
  linux: {
    // AppImage: any distribution, updates itself. deb (Debian, Ubuntu, Mint…) and rpm (Fedora, openSUSE…): installed
    // with the system's package manager, with a shortcut in the app menu; new versions are installed by hand.
    // The Build workflow makes each one for x64 and arm64.
    target: ['AppImage', 'deb', 'rpm'],
    category: 'Network',
    // deb and rpm: package maintainer (required by the deb format).
    maintainer: 'Christophe Vandevoir <115950432+ItsKarmaOff@users.noreply.github.com>',
    icon: 'resources/icons/aurore.png',
    executableName: 'signet',
    executableArgs: ['--ozone-platform=x11'],
    artifactName: '${productName}-${version}-${arch}.${ext}'
  },
  win: {
    target: ['nsis'],
    icon: 'resources/icons/aurore.png',
    artifactName: '${productName}-${version}-${arch}-setup.${ext}'
  },
  protocols: [
    {
      name: 'Web',
      schemes: ['http', 'https']
    },
    {
      name: 'Signet App',
      schemes: ['signet']
    }
  ],
  extraResources: [
    {
      from: 'resources/icons',
      to: 'icons',
      filter: ['*.png']
    }
  ],
  nsis: {
    oneClick: false,
    allowToChangeInstallationDirectory: true
  }
}
