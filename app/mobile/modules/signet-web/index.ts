// Local `signet-web` module: native content blocking (iOS) and site data clearing.
// Nothing crashes without the module (Expo Go, Android): `SignetWeb` is then `null`.
export { SignetWeb, type SignetWebNative, type CompiledList, type WebsiteDataType } from './src/SignetWebModule'
export { attachContentBlocker, nativeBlockingAvailable, prepareContentBlocker, useContentBlocker } from './src/contentBlocker'
