import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ScreenColorPicker } from '@/components/ScreenColorPicker'
import { ColorPicker } from '@/components/ui/color-picker'
import { useColorPickStore } from '@/lib/colorPick'

vi.mock('@/lib/colorPick', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/colorPick')>()
  return { ...actual, sampleCanvasColor: vi.fn(() => '#12AB34') }
})

function setup() {
  const onChange = vi.fn()
  render(
    <>
      <ColorPicker
        value='#000000'
        onChange={onChange}
        triggerTestId='color-trigger'
        pickButtonTestId='color-pick'
      />
      <ScreenColorPicker />
    </>,
  )
  return onChange
}

describe('ScreenColorPicker', () => {
  beforeEach(() => {
    useColorPickStore.getState().cancel()
  })

  it('applies the colour clicked on the page', async () => {
    const onChange = setup()
    await userEvent.click(screen.getByTestId('color-trigger'))
    await userEvent.click(await screen.findByTestId('color-pick'))

    const catcher = await screen.findByTestId('screen-color-picker')
    fireEvent.pointerUp(catcher, { button: 0, clientX: 10, clientY: 10 })
    fireEvent.click(catcher)

    await waitFor(() => expect(onChange).toHaveBeenCalledWith('#12AB34'))
    expect(screen.queryByTestId('screen-color-picker')).toBeNull()
  })

  it('cancels with Escape without changing the colour', async () => {
    const onChange = setup()
    await userEvent.click(screen.getByTestId('color-trigger'))
    await userEvent.click(await screen.findByTestId('color-pick'))
    await screen.findByTestId('screen-color-picker')

    fireEvent.keyDown(window, { key: 'Escape' })

    await waitFor(() => expect(screen.queryByTestId('screen-color-picker')).toBeNull())
    expect(onChange).not.toHaveBeenCalled()
    // The colour popover stays open.
    expect(screen.getByTestId('color-pick')).toBeInTheDocument()
  })
})
