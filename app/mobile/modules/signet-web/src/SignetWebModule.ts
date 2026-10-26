import { requireOptionalNativeModule } from 'expo'

export type WebsiteDataType = 'cookies' | 'cache' | 'storage' | 'all'

export interface CompiledList {
  identifier: string
  count: number
}

/** Interface of the native `SignetWeb` module (iOS only; absent in Expo Go and on Android). */
export interface SignetWebNative {
  compileBundledRules(): Promise<CompiledList[]>
  compileRules(identifier: string, json: string): Promise<void>
  removeRules(identifier: string): Promise<void>
  attachRules(viewTag: number, identifiers: string[]): Promise<boolean>
  detachRules(viewTag: number, identifiers: string[]): Promise<boolean>
  clearWebsiteData(types: WebsiteDataType[], since?: number): Promise<void>
  websiteDataSummary(): Promise<{ records: number }>
}

export const SignetWeb = requireOptionalNativeModule<SignetWebNative>('SignetWeb')
