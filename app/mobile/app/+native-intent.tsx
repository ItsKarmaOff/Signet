// System intents (Android: http/https link opened with Signet as the default browser). Expo Router receives the full address,
// which matches no route: we redirect it to /open?url=...; `signet://...` links keep their path.
export function redirectSystemPath({ path }: { path: string; initial: boolean }): string {
  return /^https?:\/\//i.test(path) ? `/open?url=${encodeURIComponent(path)}` : path
}
