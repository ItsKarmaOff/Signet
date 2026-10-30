import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Session } from 'electron'
import { PermissionGrants, declaredOf, parseRequest, patternCovers } from './permissions'

let dir: string
let file: string
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), 'signet-permissions-'))
  file = join(dir, 'extension-permissions.json')
})
afterEach(() => rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 50 }))

const A = 'a'.repeat(32)
const B = 'b'.repeat(32)
const ses = {} as Session
const caller = (extensionId = A) => ({ ses, extensionId })
const manifest = {
  permissions: ['storage', 'alarms'],
  optional_permissions: ['cookies', 'nativeMessaging'],
  host_permissions: ['https://required.example/*'],
  optional_host_permissions: ['*://*/*']
}

function make(answer = true) {
  const emit = vi.fn()
  const grants = new PermissionGrants(file, { emit })
  const prompt = vi.fn(() => Promise.resolve(answer))
  grants.prompt = prompt
  return { grants, emit, prompt }
}

describe('patternCovers', () => {
  it('matches schemes, hosts, ports and paths', () => {
    expect(patternCovers('<all_urls>', 'https://a.example/*')).toBe(true)
    expect(patternCovers('<all_urls>', 'chrome-extension://x/*')).toBe(false)
    expect(patternCovers('*://*/*', 'https://a.example/x')).toBe(true)
    expect(patternCovers('*://*/*', '<all_urls>')).toBe(false)
    expect(patternCovers('https://*.example.com/*', 'https://a.b.example.com/*')).toBe(true)
    expect(patternCovers('https://*.example.com/*', 'https://example.com/*')).toBe(true)
    expect(patternCovers('https://*.example.com/*', 'https://*.com/*')).toBe(false)
    expect(patternCovers('https://*.example.com/*', 'http://a.example.com/*')).toBe(false)
    expect(patternCovers('https://a.example/*', 'https://*.example/*')).toBe(false)
    expect(patternCovers('https://a.example/*', 'https://a.example:8080/*')).toBe(true)
    expect(patternCovers('https://a.example:8080/*', 'https://a.example/*')).toBe(false)
    expect(patternCovers('https://a.example/app/*', 'https://a.example/app/x')).toBe(true)
    expect(patternCovers('https://a.example/app/*', 'https://a.example/*')).toBe(false)
    expect(patternCovers('*://a.example/*', 'ftp://a.example/*')).toBe(false)
  })
})

describe('declaredOf and parseRequest', () => {
  it('splits API permissions from hosts, including MV2 hosts in permissions', () => {
    expect(declaredOf({ permissions: ['tabs', 'https://x.example/*'], optional_permissions: ['<all_urls>', 'cookies'] })).toEqual({
      required: { permissions: ['tabs'], origins: ['https://x.example/*'] },
      optional: { permissions: ['cookies'], origins: ['<all_urls>'] }
    })
  })
  it('rejects malformed requests', () => {
    expect(() => parseRequest(null)).toThrow(/Invalid arguments/)
    expect(() => parseRequest({ permissions: 'tabs' })).toThrow(/Invalid arguments/)
    expect(() => parseRequest({ origins: ['nonsense'] })).toThrow(/Invalid host permission pattern/)
    expect(parseRequest({ permissions: ['a', 'a'] })).toEqual({ permissions: ['a'], origins: [] })
  })
})

describe('request', () => {
  it('refuses what the manifest does not declare', async () => {
    const { grants, prompt } = make()
    expect(() => grants.request(caller(), manifest, { permissions: ['history'] })).toThrow('Only permissions specified in the manifest may be requested.')
    const narrow = { optional_host_permissions: ['https://only.example/*'] }
    expect(() => grants.request(caller(), narrow, { origins: ['https://other.example/*'] })).toThrow(/Only permissions specified/)
    expect(prompt).not.toHaveBeenCalled()
  })

  it('resolves true without a dialog when everything is held already', async () => {
    const { grants, prompt } = make()
    expect(await grants.request(caller(), manifest, { permissions: ['storage'], origins: ['https://required.example/page'] })).toBe(true)
    expect(prompt).not.toHaveBeenCalled()
  })

  it('asks once for the new items only, stores them and emits onAdded', async () => {
    const { grants, emit, prompt } = make()
    expect(
      await grants.request(caller(), manifest, { permissions: ['storage', 'cookies'], origins: ['https://required.example/*', 'https://new.example/*'] })
    ).toBe(true)
    expect(prompt).toHaveBeenCalledOnce()
    expect(prompt).toHaveBeenCalledWith(caller(), { permissions: ['cookies'], origins: ['https://new.example/*'] })
    expect(emit).toHaveBeenCalledWith(A, 'permissions.onAdded', [{ permissions: ['cookies'], origins: ['https://new.example/*'] }], true)
    expect(grants.permissionsOf(A)).toEqual(['cookies'])
    expect(grants.originsOf(A)).toEqual(['https://new.example/*'])
  })

  it('resolves false and stores nothing when denied', async () => {
    const { grants, emit } = make(false)
    expect(await grants.request(caller(), manifest, { permissions: ['cookies'] })).toBe(false)
    expect(grants.permissionsOf(A)).toEqual([])
    expect(emit).not.toHaveBeenCalled()
  })

  it('queues requests of one extension: no two dialogs at once, and the second only asks for what is still missing', async () => {
    const { grants, prompt } = make()
    let open = 0
    let maxOpen = 0
    let release = () => {}
    prompt.mockImplementation(() => {
      open++
      maxOpen = Math.max(maxOpen, open)
      return new Promise<boolean>((resolve) => {
        release = () => {
          open--
          resolve(true)
        }
      })
    })
    const first = grants.request(caller(), manifest, { permissions: ['cookies'] })
    const second = grants.request(caller(), manifest, { permissions: ['cookies', 'nativeMessaging'] })
    await Promise.resolve()
    await Promise.resolve()
    expect(prompt).toHaveBeenCalledTimes(1)
    release()
    await first
    await vi.waitFor(() => expect(prompt).toHaveBeenCalledTimes(2))
    expect(prompt).toHaveBeenLastCalledWith(caller(), { permissions: ['nativeMessaging'], origins: [] })
    release()
    expect(await second).toBe(true)
    expect(maxOpen).toBe(1)
  })

  it('a failing dialog does not block the queue', async () => {
    const { grants, prompt } = make()
    prompt.mockRejectedValueOnce(new Error('boom'))
    await expect(grants.request(caller(), manifest, { permissions: ['cookies'] })).rejects.toThrow('boom')
    expect(await grants.request(caller(), manifest, { permissions: ['cookies'] })).toBe(true)
  })
})

describe('contains and getAll', () => {
  it('counts required items, grants and covered origins', async () => {
    const { grants } = make()
    expect(grants.contains(manifest, A, { permissions: ['cookies'] })).toBe(false)
    expect(grants.contains(manifest, A, { permissions: ['storage'], origins: ['https://required.example/x'] })).toBe(true)
    await grants.request(caller(), manifest, { permissions: ['cookies'], origins: ['*://*/*'] })
    expect(grants.contains(manifest, A, { permissions: ['cookies', 'alarms'], origins: ['https://anything.example/*'] })).toBe(true)
    expect(grants.getAll(manifest, A)).toEqual({
      permissions: ['storage', 'alarms', 'cookies'],
      origins: ['https://required.example/*', '*://*/*']
    })
    expect(grants.getAll(manifest, B).permissions).toEqual(['storage', 'alarms'])
  })
})

describe('remove', () => {
  it('rejects required permissions and hosts, even mixed with optional ones', async () => {
    const { grants, emit } = make()
    await grants.request(caller(), manifest, { permissions: ['cookies'] })
    emit.mockClear()
    expect(() => grants.remove(manifest, A, { permissions: ['storage'] })).toThrow('You cannot remove required permissions.')
    expect(() => grants.remove(manifest, A, { origins: ['https://required.example/*'] })).toThrow('You cannot remove required permissions.')
    expect(() => grants.remove(manifest, A, { permissions: ['cookies', 'alarms'] })).toThrow('You cannot remove required permissions.')
    expect(grants.permissionsOf(A)).toEqual(['cookies'])
    expect(emit).not.toHaveBeenCalled()
  })

  it('deletes optional grants, emits onRemoved and resolves true; unheld items resolve true quietly', async () => {
    const { grants, emit } = make()
    await grants.request(caller(), manifest, { permissions: ['cookies', 'nativeMessaging'] })
    emit.mockClear()
    expect(grants.remove(manifest, A, { permissions: ['cookies', 'history'] })).toBe(true)
    expect(grants.permissionsOf(A)).toEqual(['nativeMessaging'])
    expect(emit).toHaveBeenCalledWith(A, 'permissions.onRemoved', [{ permissions: ['cookies'], origins: [] }], true)
    emit.mockClear()
    expect(grants.remove(manifest, A, { permissions: ['cookies'] })).toBe(true)
    expect(emit).not.toHaveBeenCalled()
  })
})

describe('persistence and lifecycle', () => {
  it('writes only runtime grants, in the documented shape, and reads them back', async () => {
    const { grants } = make()
    await grants.request(caller(), manifest, { permissions: ['cookies'], origins: ['https://new.example/*'] })
    expect(grants.flush()).toBe(true)
    expect(JSON.parse(readFileSync(file, 'utf8'))).toEqual({ [A]: { permissions: ['cookies'], origins: ['https://new.example/*'] } })
    const again = new PermissionGrants(file, { emit: vi.fn() })
    expect(again.permissionsOf(A)).toEqual(['cookies'])
    expect(again.originsOf(A)).toEqual(['https://new.example/*'])
  })

  it('ignores a corrupt file', () => {
    writeFileSync(file, '{nope')
    expect(new PermissionGrants(file, { emit: vi.fn() }).permissionsOf(A)).toEqual([])
  })

  it('drop removes the grants of an uninstalled extension and notifies', async () => {
    const { grants } = make()
    const changed = vi.fn()
    await grants.request(caller(), manifest, { permissions: ['cookies'] })
    grants.onChange(changed)
    grants.drop(A)
    expect(changed).toHaveBeenCalledWith(A)
    expect(grants.permissionsOf(A)).toEqual([])
    grants.flush()
    expect(JSON.parse(readFileSync(file, 'utf8'))).toEqual({})
    grants.drop(A)
    expect(changed).toHaveBeenCalledTimes(1)
  })

  it('reconcile drops grants the manifest no longer lists as optional, or now requires', async () => {
    const { grants } = make()
    await grants.request(caller(), manifest, { permissions: ['cookies', 'nativeMessaging'], origins: ['https://new.example/*'] })
    const changed = vi.fn()
    grants.onChange(changed)
    grants.reconcile(A, manifest)
    expect(changed).not.toHaveBeenCalled()
    grants.reconcile(A, { permissions: ['nativeMessaging'], optional_permissions: ['cookies'], optional_host_permissions: ['https://other.example/*'] })
    expect(changed).toHaveBeenCalledWith(A)
    expect(grants.permissionsOf(A)).toEqual(['cookies'])
    expect(grants.originsOf(A)).toEqual([])
    grants.reconcile(A, {})
    expect(grants.permissionsOf(A)).toEqual([])
    grants.flush()
    expect(existsSync(file)).toBe(true)
    expect(JSON.parse(readFileSync(file, 'utf8'))).toEqual({})
  })
})

describe('methods', () => {
  it('read the manifest of the calling extension from its session', async () => {
    const { grants } = make()
    const session = { extensions: { getExtension: (id: string) => (id === A ? { manifest } : null) } } as unknown as Session
    const m = grants.methods()
    const c = { ses: session, extensionId: A, context: undefined as never }
    expect(await m['permissions.request'](c, { permissions: ['cookies'] })).toBe(true)
    expect(m['permissions.contains'](c, { permissions: ['cookies'] })).toBe(true)
    expect(m['permissions.getAll'](c)).toMatchObject({ permissions: ['storage', 'alarms', 'cookies'] })
    expect(m['permissions.remove'](c, { permissions: ['cookies'] })).toBe(true)
  })
})
