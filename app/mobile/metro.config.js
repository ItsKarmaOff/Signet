// npm workspaces monorepo: Expo detects the root and watches packages/shared on its own (SDK 52+).
// One precaution only: `react` must be the mobile app's (the one from react-native), not the desktop's
// hoisted to the repo root, otherwise two copies of React end up in the bundle.
const path = require('node:path')
const { getDefaultConfig } = require('expo/metro-config')

const config = getDefaultConfig(__dirname)

const mobileReact = path.dirname(require.resolve('react/package.json', { paths: [__dirname] }))

const defaultResolve = config.resolver.resolveRequest
config.resolver.resolveRequest = (context, moduleName, platform) => {
  if (moduleName === 'react' || moduleName.startsWith('react/')) {
    const sub = moduleName.slice('react'.length)
    return context.resolveRequest(context, mobileReact + sub, platform)
  }
  return (defaultResolve ?? context.resolveRequest)(context, moduleName, platform)
}

module.exports = config
