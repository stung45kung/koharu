import { describe, expect, it } from 'vitest'

import { pickClick, pickState, setMany } from '@/lib/pickSet'

const ids = ['a', 'b', 'c', 'd', 'e']
const sorted = (s: Set<string>) => [...s].sort()

describe('pickSet', () => {
  it('ticks and unticks many ids without touching others', () => {
    expect(sorted(setMany(new Set(['x']), ['a', 'b'], true))).toEqual(['a', 'b', 'x'])
    expect(sorted(setMany(new Set(['a', 'b', 'x']), ['a', 'b'], false))).toEqual(['x'])
  })

  it('reports all / some / none of the shown ids', () => {
    expect(pickState(new Set(['a', 'b']), ['a', 'b'])).toBe('all')
    expect(pickState(new Set(['a']), ['a', 'b'])).toBe('some')
    expect(pickState(new Set(['x']), ['a', 'b'])).toBe('none')
    expect(pickState(new Set(), [])).toBe('none')
  })

  it('a plain click flips one item', () => {
    expect(sorted(pickClick(new Set(['a']), ids, 'c', false, 'a'))).toEqual(['a', 'c'])
    expect(sorted(pickClick(new Set(['a', 'c']), ids, 'c', false, 'a'))).toEqual(['a'])
  })

  it('Shift+click ticks the range from the last clicked item, either direction', () => {
    expect(sorted(pickClick(new Set(['b']), ids, 'd', true, 'b'))).toEqual(['b', 'c', 'd'])
    expect(sorted(pickClick(new Set(['d']), ids, 'b', true, 'd'))).toEqual(['b', 'c', 'd'])
  })

  it('Shift+click on a ticked item unticks the range', () => {
    expect(sorted(pickClick(new Set(ids), ids, 'd', true, 'b'))).toEqual(['a', 'e'])
  })

  it('Shift+click without an anchor in the list flips one item', () => {
    expect(sorted(pickClick(new Set(), ids, 'c', true, null))).toEqual(['c'])
    expect(sorted(pickClick(new Set(), ids, 'c', true, 'gone'))).toEqual(['c'])
  })
})
