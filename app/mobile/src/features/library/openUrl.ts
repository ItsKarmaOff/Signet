import type { useRouter } from 'expo-router'
import { isLoopbackUrl } from '@signet/shared/link/loopback'
import { resolveLocalhostUrl } from '@/features/link/localhost'
import { useStore } from '@/state'

/** Opens an address in a new tab (private when in private browsing), then closes the library to return to the browser. */
export function openInBrowser(router: ReturnType<typeof useRouter>, url: string): void {
  // Local address: with a connected computer, it goes through it (any choice is made before leaving the screen).
  if (isLoopbackUrl(url)) {
    void resolveLocalhostUrl(url).then((target) => {
      if (!target) return
      useStore.getState().openTab({ url: target })
      router.dismissTo('/')
    })
    return
  }
  useStore.getState().openTab({ url })
  router.dismissTo('/')
}
