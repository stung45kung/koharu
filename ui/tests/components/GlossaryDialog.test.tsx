import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { GlossaryDialog } from '@/components/GlossaryDialog'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

describe('GlossaryDialog', () => {
  const puts: unknown[] = []

  beforeEach(() => {
    puts.length = 0
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json({ epoch: 0, scene: { pages: {}, project: { name: 'P' } } }),
      ),
      http.get('/api/v1/glossary', () =>
        HttpResponse.json({ entries: [{ source: 'すい', target: 'ซุย', note: '' }] }),
      ),
      http.put('/api/v1/glossary', async ({ request }) => {
        const body = await request.json()
        puts.push(body)
        return HttpResponse.json(body)
      }),
    )
  })

  it('edits, adds and saves terms, dropping blank rows', async () => {
    const onOpenChange = vi.fn()
    renderWithQuery(<GlossaryDialog open onOpenChange={onOpenChange} />)

    await waitFor(() => expect(screen.getByTestId('glossary-source-0')).toHaveValue('すい'))
    await userEvent.click(screen.getByTestId('glossary-add'))
    await userEvent.type(screen.getByTestId('glossary-source-1'), '旅館')
    await userEvent.type(screen.getByTestId('glossary-target-1'), 'เรียวกัง')
    await userEvent.click(screen.getByTestId('glossary-add'))
    await userEvent.click(screen.getByTestId('glossary-save'))

    await waitFor(() => expect(puts).toHaveLength(1))
    expect(puts[0]).toEqual({
      entries: [
        { source: 'すい', target: 'ซุย', note: '' },
        { source: '旅館', target: 'เรียวกัง', note: '' },
      ],
    })
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
  })

  it('deletes a term', async () => {
    renderWithQuery(<GlossaryDialog open onOpenChange={() => {}} />)
    await waitFor(() => expect(screen.getByTestId('glossary-source-0')).toHaveValue('すい'))
    await userEvent.click(screen.getByTestId('glossary-delete-0'))
    expect(screen.queryByTestId('glossary-row-0')).toBeNull()
  })
})
