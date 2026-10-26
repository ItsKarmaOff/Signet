import { useEffect, useMemo, useState } from 'react'
import { StyleSheet, View } from 'react-native'
import { Image } from 'expo-image'
import { accentColor, tintColor, withAlpha } from '@signet/shared/mobile/color'
import { iconChain } from '@signet/shared/mobile/favicons'
import { siteOf } from '@signet/shared/site'
import { useTheme } from './theme'
import { Text } from './Text'
import { Globe } from './icons'
import { radius } from './tokens'

export interface FaviconProps {
  /** Address of the site's icon, if known. */
  uri?: string
  /** Page address: used for the fallback letter and its color. */
  url?: string
  size?: number
}

const hashHue = (s: string) => {
  let h = 0
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 360
  return h
}

/**
 * Site icon. Fallback chain: the icon kept for the page, then the site's `/apple-touch-icon.png`, then DuckDuckGo, then a letter
 * on a tinted background (stable color per site, like desktop). The letter stays visible until the image loads: nothing blocks rendering.
 * The disk cache is expo-image's.
 */
export function Favicon({ uri, url = '', size = 28 }: FaviconProps) {
  const { scheme } = useTheme()
  const chain = useMemo(() => iconChain(uri, url), [uri, url])
  const key = chain.join('|')
  const [index, setIndex] = useState(0)
  const [loaded, setLoaded] = useState(false)
  useEffect(() => {
    setIndex(0)
    setLoaded(false)
  }, [key])
  const site = siteOf(url) || url.replace(/^\w+:\/\//, '')
  const box = { width: size, height: size, borderRadius: Math.min(radius.tile, size * 0.3) }
  const current = chain[index]
  const hue = hashHue(site)
  return (
    <View style={[styles.fallback, box, !loaded && { backgroundColor: withAlpha(tintColor(hue, scheme), 0.22) }]}>
      {!loaded ? (
        site ? (
          <Text variant="caption" weight="700" color={accentColor(hue, scheme)} style={{ fontSize: Math.round(size * 0.46), lineHeight: size }}>
            {site[0].toUpperCase()}
          </Text>
        ) : (
          <Globe size={Math.round(size * 0.6)} color={accentColor(hue, scheme)} strokeWidth={2} />
        )
      ) : null}
      {current ? (
        <Image
          key={current}
          source={{ uri: current }}
          style={[StyleSheet.absoluteFill, box]}
          contentFit="cover"
          cachePolicy="disk"
          recyclingKey={current}
          onLoad={() => setLoaded(true)}
          onError={() => setIndex((i) => i + 1)}
          accessibilityIgnoresInvertColors
        />
      ) : null}
    </View>
  )
}

const styles = StyleSheet.create({ fallback: { alignItems: 'center', justifyContent: 'center', overflow: 'hidden' } })
