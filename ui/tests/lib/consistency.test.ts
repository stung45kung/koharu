import { describe, expect, it } from 'vitest'

import { checkConsistency } from '@/lib/consistency'

describe('checkConsistency', () => {
  const glossary = [{ source: 'ルルイエ', target: 'รุรุเอะ', note: '' }]

  it('flags glossary misses, untranslated blocks and differing renderings', () => {
    const issues = checkConsistency(
      [
        {
          id: 'p1',
          blocks: [
            { id: 'a', text: 'ルルイエ、来て', translation: 'รูรูเอะ มานี่' },
            { id: 'b', text: 'ありがとう', translation: 'ขอบใจนะ' },
            { id: 'c', text: '……', translation: '' },
          ],
        },
        {
          id: 'p2',
          blocks: [
            { id: 'd', text: 'ありがとう', translation: 'ขอบคุณค่ะ' },
            { id: 'e', text: 'ルルイエ!', translation: 'รุรุเอะ!' },
            { id: 'f', text: '  ', translation: '' },
          ],
        },
      ],
      glossary,
    )
    const summary = issues.map((i) => `${i.kind}:${i.pageNumber}.${i.blockNumber}`)
    expect(summary).toEqual([
      'glossary:1.1',
      'untranslated:1.3',
      'inconsistent:1.2',
      'inconsistent:2.1',
    ])
    expect(issues[0].detail).toBe('ルルイエ → รุรุเอะ')
    expect(issues[2].detail).toBe('ขอบคุณค่ะ')
  })

  it('treats whitespace differences as the same text', () => {
    const issues = checkConsistency(
      [
        {
          id: 'p1',
          blocks: [
            { id: 'a', text: 'ありが とう', translation: 'ขอบใจ  นะ' },
            { id: 'b', text: 'ありがとう', translation: 'ขอบใจ นะ' },
          ],
        },
      ],
      [],
    )
    expect(issues).toEqual([])
  })
})
