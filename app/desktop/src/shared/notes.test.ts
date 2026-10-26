import { describe, expect, it } from 'vitest'
import { EASEL_MAX, NOTE_ID, NOTE_MAX, noteAddress, parseNoteAddress } from './notes'

const ID = '123e4567-e89b-12d3-a456-426614174000'

describe('NOTE_ID', () => {
  it('recognizes a UUID', () => {
    expect(NOTE_ID.test(ID)).toBe(true)
    expect(NOTE_ID.test(ID.toUpperCase())).toBe(true)
    expect(NOTE_ID.test('not-a-uuid')).toBe(false)
    expect(NOTE_ID.test(`${ID}x`)).toBe(false)
  })
})

describe('adresses de documents', () => {
  it('noteAddress writes signet://<kind>/<id>', () => {
    expect(noteAddress('note', ID)).toBe(`signet://note/${ID}`)
    expect(noteAddress('easel', ID)).toBe(`signet://easel/${ID}`)
  })

  it('parseNoteAddress fait l’aller-retour', () => {
    expect(parseNoteAddress(noteAddress('note', ID))).toEqual({ kind: 'note', id: ID })
    expect(parseNoteAddress(noteAddress('easel', ID))).toEqual({ kind: 'easel', id: ID })
  })

  it('tolerates case, spaces and a trailing /, and lowercases', () => {
    expect(parseNoteAddress(`  SIGNET://Note/${ID.toUpperCase()}/ `)).toEqual({ kind: 'note', id: ID })
  })

  it('rejects other addresses', () => {
    for (const bad of [
      '',
      'https://example.com',
      `signet://other/${ID}`,
      `signet://note/${ID}/extra`,
      'signet://note/123',
      `signet://note/${'z'.repeat(36)}`
    ]) {
      expect(parseNoteAddress(bad), bad).toBeNull()
    }
  })
})

it('limites de taille', () => {
  expect(NOTE_MAX).toBe(2 * 1024 * 1024)
  expect(EASEL_MAX).toBe(30 * 1024 * 1024)
})
