// Opens an address received from another app (Android: Signet chosen as the browser, VIEW http/https intent). See +native-intent.tsx.
// Opens a tab on the address (in the active space) then redirects to the browser. Only accepts http and https.
import { useEffect } from 'react'
import { Redirect, useLocalSearchParams } from 'expo-router'
import { useStore } from '@/state'

export default function OpenFromSystem() {
  const { url } = useLocalSearchParams<{ url?: string }>()
  useEffect(() => {
    if (url && /^https?:\/\//i.test(url)) useStore.getState().openTab({ url })
  }, [url])
  return <Redirect href="/" />
}
