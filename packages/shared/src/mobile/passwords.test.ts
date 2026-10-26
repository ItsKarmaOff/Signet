import { describe, expect, it } from 'vitest'
import {
  addNever,
  credentialsForPage,
  exportPasswordCsv,
  isNever,
  matchesPage,
  normalizeOrigin,
  parseCsv,
  parsePasswordCsv,
  planImport,
  planSave,
  type Credential
} from './passwords'

const cred = (over: Partial<Credential>): Credential => ({
  id: 'x',
  origin: 'https://example.com',
  username: 'ana',
  password: 'pw',
  createdAt: 1,
  lastUsed: null,
  ...over
})

describe('normalizeOrigin', () => {
  it('free-form addresses and inputs -> http(s) origin', () => {
    expect(normalizeOrigin('example.com')).toBe('https://example.com')
    expect(normalizeOrigin('https://Example.com/login?x=1')).toBe('https://example.com')
    expect(normalizeOrigin('http://localhost:3000/a')).toBe('http://localhost:3000')
  })
  it('refuses the rest', () => {
    for (const bad of ['', '  ', 'ftp://a.com', 'javascript:alert(1)', 'a b', 'file:///x']) expect(normalizeOrigin(bad)).toBeNull()
  })
})

describe('matchesPage', () => {
  it('same origin, give or take www.', () => {
    expect(matchesPage('https://example.com', 'https://example.com/login')).toBe(true)
    expect(matchesPage('https://www.example.com', 'https://example.com/')).toBe(true)
    expect(matchesPage('https://example.com', 'https://www.example.com/')).toBe(true)
  })
  it('saved domain -> subdomains', () => {
    expect(matchesPage('https://example.com', 'https://accounts.example.com/')).toBe(true)
    expect(matchesPage('https://accounts.example.com', 'https://example.com/')).toBe(false)
    expect(matchesPage('https://a.example.com', 'https://b.example.com/')).toBe(false)
  })
  it('never another domain nor a public suffix', () => {
    expect(matchesPage('https://example.com', 'https://notexample.com/')).toBe(false)
    expect(matchesPage('https://example.com', 'https://example.com.evil.io/')).toBe(false)
    expect(matchesPage('https://github.io', 'https://evil.github.io/')).toBe(false)
    expect(matchesPage('https://co.uk', 'https://evil.co.uk/')).toBe(false)
  })
  it('http: identical origin only', () => {
    expect(matchesPage('https://example.com', 'http://example.com/')).toBe(false)
    expect(matchesPage('https://example.com', 'http://sub.example.com/')).toBe(false)
    expect(matchesPage('http://localhost:3000', 'http://localhost:3000/x')).toBe(true)
    expect(matchesPage('http://localhost:3000', 'http://localhost:4000/x')).toBe(false)
  })
  it('invalid or non-web addresses', () => {
    expect(matchesPage('https://example.com', 'about:blank')).toBe(false)
    expect(matchesPage('nope', 'https://example.com')).toBe(false)
  })
})

describe('credentialsForPage', () => {
  it('filters and ranks by last use', () => {
    const list = [
      cred({ id: 'a', username: 'old', lastUsed: 5 }),
      cred({ id: 'b', username: 'new', lastUsed: 9 }),
      cred({ id: 'c', origin: 'https://other.com' }),
      cred({ id: 'd', username: 'never', createdAt: 7 })
    ]
    expect(credentialsForPage(list, 'https://example.com/').map((c) => c.id)).toEqual(['b', 'd', 'a'])
  })
})

describe('planSave', () => {
  const existing = [cred({ id: 'a', username: 'Ana', password: 'one' })]
  const sub = { origin: 'https://example.com', username: 'ana', password: 'one' }
  it('identique -> rien', () => expect(planSave(existing, sub)).toEqual({ action: 'none' }))
  it('other password -> update', () => expect(planSave(existing, { ...sub, password: 'two' })).toEqual({ action: 'update', id: 'a' }))
  it('other username or other site -> new', () => {
    expect(planSave(existing, { ...sub, username: 'bob' })).toEqual({ action: 'save' })
    expect(planSave(existing, { ...sub, origin: 'https://other.com' })).toEqual({ action: 'save' })
    expect(planSave([], sub)).toEqual({ action: 'save' })
  })
  it('without username: known -> nothing, otherwise new', () => {
    expect(planSave(existing, { ...sub, username: '' })).toEqual({ action: 'none' })
    expect(planSave(existing, { ...sub, username: '', password: 'zzz' })).toEqual({ action: 'save' })
  })
  it('empty or too long password -> nothing', () => {
    expect(planSave([], { ...sub, password: '' })).toEqual({ action: 'none' })
    expect(planSave([], { ...sub, password: 'x'.repeat(1001) })).toEqual({ action: 'none' })
  })
})

describe('CSV', () => {
  it('quotes, commas, line breaks, BOM', () => {
    expect(parseCsv('﻿a,b\r\n"x,1","he said ""hi"""\n"l1\nl2",z\n\n')).toEqual([
      ['a', 'b'],
      ['x,1', 'he said "hi"'],
      ['l1\nl2', 'z']
    ])
  })
  it('import Chrome / Apple Mots de passe / Firefox', () => {
    const chrome = 'name,url,username,password,note\nex,https://example.com/login,ana,"p,w",\n'
    expect(parsePasswordCsv(chrome)).toEqual([{ origin: 'https://example.com', username: 'ana', password: 'p,w' }])
    const apple = 'Title,URL,Username,Password,Notes,OTPAuth\nEx,example.org,bob,secret,,\n'
    expect(parsePasswordCsv(apple)).toEqual([{ origin: 'https://example.org', username: 'bob', password: 'secret' }])
    const firefox = '"url","username","password","httpRealm"\n"https://a.com","c","d",""\n'
    expect(parsePasswordCsv(firefox)).toEqual([{ origin: 'https://a.com', username: 'c', password: 'd' }])
  })
  it('ignores rows without a web address or without a password; unknown format -> null', () => {
    expect(parsePasswordCsv('url,username,password\nftp://a.com,a,b\nhttps://a.com,a,\nhttps://b.com,a,b\n')).toHaveLength(1)
    expect(parsePasswordCsv('foo,bar\n1,2\n')).toBeNull()
    expect(parsePasswordCsv('')).toBeNull()
  })
  it('export then import: same credentials', () => {
    const list = [cred({ username: 'a,b', password: 'q"uote' }), cred({ origin: 'http://localhost:3000', username: '', password: 'x\ny' })]
    const csv = exportPasswordCsv(list)
    expect(csv.split('\n')[0]).toBe('name,url,username,password,note')
    expect(parsePasswordCsv(csv)).toEqual(list.map((c) => ({ origin: c.origin, username: c.username, password: c.password })))
  })
})

describe('planImport', () => {
  it('adds the new ones, counts the existing ones, first duplicate wins', () => {
    const have = [cred({ username: 'ana' })]
    const rows = [
      { origin: 'https://example.com', username: 'Ana', password: 'p' },
      { origin: 'https://example.com', username: 'ana', password: 'p2' },
      { origin: 'https://new.com', username: 'u', password: 'first' },
      { origin: 'https://new.com', username: 'u', password: 'second' }
    ]
    const plan = planImport(have, rows)
    expect(plan.existing).toBe(1)
    expect(plan.toAdd).toEqual([rows[2]])
  })
})

describe('never save', () => {
  it('list without duplicates, matching like for credentials', () => {
    const list = addNever(addNever([], 'https://bank.com'), 'https://bank.com')
    expect(list).toEqual(['https://bank.com'])
    expect(isNever(list, 'https://bank.com/login')).toBe(true)
    expect(isNever(list, 'https://www.bank.com/')).toBe(true)
    expect(isNever(list, 'https://other.com/')).toBe(false)
  })
})
