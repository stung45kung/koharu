import { screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'

import { LibraryDialog } from '@/components/LibraryDialog'
import { queryClient } from '@/lib/queryClient'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

const term = (source: string) => ({ source, target: `${source}-th`, note: '' })
const libTerm = (key: string, series: string) => ({
  key,
  series,
  updatedAtMs: 0,
  entry: term(key),
})

beforeEach(() => {
  queryClient.clear()
  server.use(
    http.get('/api/v1/scene.json', () =>
      HttpResponse.json({ epoch: 0, scene: { pages: {}, project: { name: 'P' } } }),
    ),
    http.get('/api/v1/glossary', () =>
      HttpResponse.json({ entries: ['a', 'b', 'c', 'd'].map(term) }),
    ),
    http.get('/api/v1/library', () =>
      HttpResponse.json({
        characters: [],
        terms: [libTerm('x1', 'X'), libTerm('x2', 'X'), libTerm('y1', 'Y')],
      }),
    ),
  )
})

async function openTerms() {
  renderWithQuery(<LibraryDialog open onOpenChange={() => {}} />)
  await userEvent.click(await screen.findByTestId('library-tab-terms'))
}

describe('LibraryDialog picking', () => {
  it('select all ticks every project term, and again unticks them', async () => {
    await openTerms()
    const all = await screen.findByTestId('library-project-all')
    await userEvent.click(all)
    expect(screen.getByTestId('library-save-selected')).toHaveTextContent('4')
    await userEvent.click(all)
    expect(screen.getByTestId('library-save-selected')).toHaveTextContent(/^$/)
  })

  it('Shift+click ticks a range of project terms', async () => {
    await openTerms()
    // One user-event instance, so the held Shift reaches the click.
    const user = userEvent.setup()
    const rows = await screen.findAllByTestId('library-project-row')
    await user.click(rows[0])
    await user.keyboard('{Shift>}')
    await user.click(rows[2])
    await user.keyboard('{/Shift}')
    const ticked = rows.map((r) => (within(r).getByRole('checkbox') as HTMLInputElement).checked)
    expect(ticked).toEqual([true, true, true, false])
  })

  it('a series tick box picks only that series in the library', async () => {
    await openTerms()
    await userEvent.click(await screen.findByTestId('library-series-all-X'))
    expect(screen.getByTestId('library-add-selected')).toHaveTextContent('2')
    await userEvent.click(screen.getByTestId('library-all'))
    expect(screen.getByTestId('library-add-selected')).toHaveTextContent('3')
  })

  it('select all in the library picks only the items the search shows', async () => {
    await openTerms()
    await screen.findByTestId('library-all')
    await userEvent.type(screen.getByPlaceholderText('library.search'), 'x')
    await userEvent.click(screen.getByTestId('library-all'))
    expect(screen.getByTestId('library-add-selected')).toHaveTextContent('2')
  })
})
