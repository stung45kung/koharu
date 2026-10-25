import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { CharacterManagerDialog } from '@/components/CharacterManagerDialog'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

const book = {
  version: 1,
  characters: [{ id: 'c1', name: 'Hood', description: 'archaic speech' }],
  speakers: { t1: 'c1', t2: 'c1' },
}

describe('CharacterManagerDialog', () => {
  beforeEach(() => {
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json({ epoch: 0, scene: { pages: {}, project: { name: 'P' } } }),
      ),
      http.get('/api/v1/characters', () => HttpResponse.json(book)),
    )
  })

  it('lists characters with their usage count', async () => {
    renderWithQuery(<CharacterManagerDialog open onOpenChange={() => {}} />)

    await waitFor(() => expect(screen.getByTestId('character-name-0')).toHaveValue('Hood'))
    expect(screen.getByTestId('character-description-0')).toHaveValue('archaic speech')
    expect(screen.getByText('characters.usedBy')).toBeInTheDocument()
  })

  it('saves a newly added character', async () => {
    const puts: unknown[] = []
    server.use(
      http.put('/api/v1/characters', async ({ request }) => {
        const body = (await request.json()) as { characters: unknown[] }
        puts.push(body)
        return HttpResponse.json({ ...book, characters: body.characters })
      }),
    )
    const onOpenChange = vi.fn()
    renderWithQuery(<CharacterManagerDialog open onOpenChange={onOpenChange} />)
    await waitFor(() => expect(screen.getByTestId('character-name-0')).toHaveValue('Hood'))

    await userEvent.click(screen.getByTestId('character-add'))
    await userEvent.type(screen.getByTestId('character-name-1'), 'Narrator')
    await userEvent.click(screen.getByTestId('character-save'))

    await waitFor(() => expect(puts).toHaveLength(1))
    expect(puts[0]).toMatchObject({
      characters: [
        { id: 'c1', name: 'Hood' },
        { name: 'Narrator', description: '' },
      ],
    })
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
  })

  it('refuses to save a character without a name', async () => {
    const puts: unknown[] = []
    server.use(
      http.put('/api/v1/characters', async ({ request }) => {
        puts.push(await request.json())
        return HttpResponse.json(book)
      }),
    )
    renderWithQuery(<CharacterManagerDialog open onOpenChange={() => {}} />)
    await waitFor(() => expect(screen.getByTestId('character-name-0')).toHaveValue('Hood'))

    await userEvent.click(screen.getByTestId('character-add'))
    await userEvent.click(screen.getByTestId('character-save'))

    expect(await screen.findByText('characters.nameRequired')).toBeInTheDocument()
    expect(puts).toHaveLength(0)
  })
})
