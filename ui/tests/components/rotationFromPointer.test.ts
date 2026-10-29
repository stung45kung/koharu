import { describe, expect, it } from 'vitest'

import { rotationFromPointer } from '@/components/canvas/TextBlockLayer'

describe('rotationFromPointer', () => {
  it('is 0 straight above the centre and grows clockwise', () => {
    expect(rotationFromPointer(0, -10, false)).toBe(0)
    expect(rotationFromPointer(10, 0, false)).toBe(90)
    expect(rotationFromPointer(-10, 0, false)).toBe(-90)
    expect(rotationFromPointer(0, 10, false)).toBe(180)
  })

  it('snaps to 15 degree steps with shift', () => {
    // ~32° clockwise → 30
    const dx = Math.sin((32 * Math.PI) / 180)
    const dy = -Math.cos((32 * Math.PI) / 180)
    expect(rotationFromPointer(dx, dy, true)).toBe(30)
    expect(rotationFromPointer(dx, dy, false)).toBeCloseTo(32, 0)
  })
})
