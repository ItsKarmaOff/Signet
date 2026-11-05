// The autofill page script as the app injects it, and the calls that reach it. The code itself is written in TypeScript
// (`page/autofill.page.ts`) and bundled into `pageScript.generated.ts` (`npm run autofill:page -w @signet/mobile`).
import { AUTOFILL_PAGE_CODE } from './pageScript.generated'
import type { AddressInput, CardNetwork } from './store'

/** Name of the fill entry point on `window`: includes the token, so only injected code can call it (non-enumerable). */
const entryName = (token: string): string => `__signetAf_${token}`

/** Injected after the page loads (main frame only). The token lives in the closure of the page script. */
export const autofillScript = (token: string): string => `(function(){try{var SIGNET_TOKEN=${JSON.stringify(token)};${AUTOFILL_PAGE_CODE}}catch(e){}})();true;`

export type FillRequest =
  | { o: string; kind: 'address'; address: AddressInput }
  | { o: string; kind: 'card'; card: { network: CardNetwork; holder: string; number: string; expMonth: number; expYear: number } }

/** Asks the page to fill the group of the last focused field; the page checks `location.origin` equals `o` and answers `af:filled`. */
export const fillCall = (token: string, req: FillRequest): string =>
  `(function(){var t=${JSON.stringify(token)};try{var f=window[${JSON.stringify(entryName(token))}];` +
  `if(f)return f(${JSON.stringify(JSON.stringify(req))});window.ReactNativeWebView.postMessage(JSON.stringify({type:'af:filled',ok:false,k:t}))}catch(e){}})();true;`
