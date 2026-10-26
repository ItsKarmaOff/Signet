// DEV ONLY: the simulator asks "Open in Signet?" again on every deep link and simctl cannot tap the screen.
// This loop reads a command every second from http://localhost:8099/cmd (QA server, see README): `{ id, path }`,
// and navigates to `path` (e.g. `/dev/seed`, `/?dev=menu`). Does nothing outside `__DEV__`.
import { useEffect, useRef } from 'react'
import { LogBox } from 'react-native'
import { useRouter } from 'expo-router'

if (__DEV__) LogBox.ignoreLogs(['onAnimatedValueUpdate', 'ADVICE'])

// Last command seen: at module level, so a command is not replayed if the component remounts.
let lastSeen: string | null = null

export function useQaCommands(): void {
  const router = useRouter()
  const routerRef = useRef(router)
  routerRef.current = router
  useEffect(() => {
    if (!__DEV__) return
    let stop = false
    const tick = async () => {
      try {
        const res = await fetch('http://localhost:8099/cmd')
        const cmd = (await res.json()) as { id?: string; path?: string }
        if (cmd.id && cmd.id !== lastSeen) {
          const first = lastSeen === null // the first read only remembers the pending command
          lastSeen = cmd.id
          if (!first && cmd.path) routerRef.current.navigate(cmd.path as never)
        }
      } catch {
        // no QA server: ignore
      }
      if (!stop) setTimeout(() => void tick(), 1000)
    }
    void tick()
    return () => {
      stop = true
    }
  }, [])
}
