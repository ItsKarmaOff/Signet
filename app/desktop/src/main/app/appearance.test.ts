import { describe, expect, it, vi } from 'vitest'

vi.mock('electron', () => ({
  app: { getPreferredSystemLanguages: () => ['en-US'], getPath: () => '' },
  webContents: { getAllWebContents: () => [] },
  nativeTheme: {}
}))

import type { Settings } from '@shared/types'
import { IS_MAC } from './platform'
import { appearanceOf } from './appearance'

describe('appearanceOf', () => {
  it('missing values: defaults', () => {
    expect(appearanceOf({} as Settings)).toEqual({ tint: 'normal', density: 'normal', text: 'normal', glass: IS_MAC })
  })
  it('known values kept, unknown ones reset to default', () => {
    const s = { tint: 'strong', density: 'roomy', textSize: 'small' } as Settings
    expect(appearanceOf(s)).toMatchObject({ tint: 'strong', density: 'roomy', text: 'small' })
    const bad = { tint: 'x', density: 'y', textSize: 'z' } as unknown as Settings
    expect(appearanceOf(bad)).toMatchObject({ tint: 'normal', density: 'normal', text: 'normal' })
  })
  it('translucency: turned off by the setting, and only on Mac', () => {
    expect(appearanceOf({ transparency: false } as Settings).glass).toBe(false)
    expect(appearanceOf({ transparency: true } as Settings).glass).toBe(IS_MAC)
  })
})
