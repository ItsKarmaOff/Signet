import { chmodSync, existsSync, mkdirSync, mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { extensionIds, profileHas } from './chromium'

let profile: string
beforeEach(() => {
  profile = mkdtempSync(join(tmpdir(), 'signet-chromium-'))
})
afterEach(() => {
  const extensions = join(profile, 'Extensions')
  if (existsSync(extensions)) chmodSync(extensions, 0o755)
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 })
})

/** An Extensions folder that can't be listed, like another browser's profile protected by macOS. */
function lockedExtensions(): void {
  mkdirSync(join(profile, 'Extensions', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'), { recursive: true })
  chmodSync(join(profile, 'Extensions'), 0o000)
}

// Windows has no such permissions, and root ignores them: the test would prove nothing there.
describe.skipIf(process.platform === 'win32' || process.getuid?.() === 0)('Chromium profile protected by the system', () => {
  it('profileHas only checks that files exist: it lists nothing and does not fail', () => {
    lockedExtensions()
    expect(profileHas(profile)).toEqual({ history: false, passwords: false, extensions: true })
  })

  it('extensionIds reports the denied access instead of "no extension"', () => {
    lockedExtensions()
    expect(() => extensionIds(profile)).toThrow(/EACCES|EPERM/)
  })
})

describe('extensionIds', () => {
  it('no Extensions folder, or an empty one: no extension', () => {
    expect(extensionIds(profile)).toEqual([])
    mkdirSync(join(profile, 'Extensions'))
    expect(extensionIds(profile)).toEqual([])
  })
})
