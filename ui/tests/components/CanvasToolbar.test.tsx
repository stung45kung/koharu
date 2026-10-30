import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'

import { CanvasToolbar } from '@/components/canvas/CanvasToolbar'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

import { renderWithQuery } from '../helpers'

describe('CanvasToolbar LLM popover', () => {
  it('keeps a long system prompt in a fixed-height box that scrolls', async () => {
    usePreferencesStore.setState({
      customSystemPrompt: Array.from({ length: 80 }, (_, i) => `rule ${i}`).join('\n'),
    })
    renderWithQuery(<CanvasToolbar />)

    await userEvent.click(screen.getByTestId('llm-trigger'))
    const prompt = await screen.findByTestId('llm-system-prompt')

    // `field-sizing: content` (the Textarea default) grows the box with
    // its text, so a long prompt ran off the window and could not scroll.
    expect(prompt.className).toContain('field-sizing-fixed')
    expect(prompt.className).not.toContain('field-sizing-content')
    expect(prompt.className).toContain('overflow-y-auto')
    expect(prompt.className).toMatch(/max-h-/)
  })
})
