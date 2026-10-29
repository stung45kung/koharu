import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { CharacterManagerDialog } from '@/components/CharacterManagerDialog'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

const book = {
  version: 1,
  characters: [
    { id: 'c1', name: 'Hood', description: 'archaic speech' },
    { id: 'c2', name: 'Narrator', description: 'neutral' },
  ],
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

  it('lists characters and edits the selected one', async () => {
    renderWithQuery(<CharacterManagerDialog open onOpenChange={() => {}} />)

    // First character is selected by default.
    await waitFor(() => expect(screen.getByTestId('character-name')).toHaveValue('Hood'))
    expect(screen.getByTestId('character-description')).toHaveValue('archaic speech')
    expect(screen.getByTestId('character-item-0')).toHaveTextContent('Hood')
    expect(screen.getByTestId('character-item-1')).toHaveTextContent('Narrator')

    await userEvent.click(screen.getByTestId('character-item-1'))
    expect(screen.getByTestId('character-name')).toHaveValue('Narrator')
    expect(screen.getByTestId('character-description')).toHaveValue('neutral')
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
    await waitFor(() => expect(screen.getByTestId('character-name')).toHaveValue('Hood'))

    await userEvent.click(screen.getByTestId('character-add'))
    // The new character is selected and empty.
    expect(screen.getByTestId('character-name')).toHaveValue('')
    await userEvent.type(screen.getByTestId('character-name'), 'Sensei')
    await userEvent.click(screen.getByTestId('character-save'))

    await waitFor(() => expect(puts).toHaveLength(1))
    expect(puts[0]).toMatchObject({
      characters: [
        { id: 'c1', name: 'Hood' },
        { id: 'c2', name: 'Narrator' },
        { name: 'Sensei', description: '' },
      ],
    })
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
  })

  it('deleting keeps a neighbour selected', async () => {
    renderWithQuery(<CharacterManagerDialog open onOpenChange={() => {}} />)
    await waitFor(() => expect(screen.getByTestId('character-name')).toHaveValue('Hood'))

    await userEvent.click(screen.getByTestId('character-delete'))
    expect(screen.getByTestId('character-name')).toHaveValue('Narrator')
    expect(screen.queryByTestId('character-item-1')).toBeNull()
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
    await waitFor(() => expect(screen.getByTestId('character-name')).toHaveValue('Hood'))

    await userEvent.click(screen.getByTestId('character-add'))
    await userEvent.click(screen.getByTestId('character-save'))

    expect(await screen.findByText('characters.nameRequired')).toBeInTheDocument()
    expect(puts).toHaveLength(0)
  })

  it('copies font and colour from the selected block and saves them', async () => {
    const puts: Array<{ characters: unknown[] }> = []
    server.use(
      http.put('/api/v1/characters', async ({ request }) => {
        const body = (await request.json()) as { characters: unknown[] }
        puts.push(body)
        return HttpResponse.json({ ...book, characters: body.characters })
      }),
    )
    renderWithQuery(
      <CharacterManagerDialog
        open
        onOpenChange={() => {}}
        selectedStyle={{ font: 'NotoSansThai-ExtraBold', color: [255, 255, 255, 255] }}
      />,
    )
    await waitFor(() => expect(screen.getByTestId('character-name')).toHaveValue('Hood'))

    await userEvent.click(screen.getByTestId('character-copy-style'))
    expect(screen.getByTestId('character-font')).toHaveValue('NotoSansThai-ExtraBold')
    expect(screen.getByTestId('character-color-value')).toHaveTextContent('#ffffff')

    await userEvent.click(screen.getByTestId('character-save'))
    await waitFor(() => expect(puts).toHaveLength(1))
    expect(puts[0].characters[0]).toMatchObject({
      id: 'c1',
      font: 'NotoSansThai-ExtraBold',
      color: [255, 255, 255, 255],
    })
  })

  it('disables copying without a selected block', async () => {
    renderWithQuery(<CharacterManagerDialog open onOpenChange={() => {}} />)
    await waitFor(() => expect(screen.getByTestId('character-name')).toHaveValue('Hood'))
    expect(screen.getByTestId('character-copy-style')).toBeDisabled()
  })
})
