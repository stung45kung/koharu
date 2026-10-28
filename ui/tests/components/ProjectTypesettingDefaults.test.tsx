import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'

import { ProjectTypesettingDefaults } from '@/components/panels/ProjectTypesettingDefaults'
import { defaultSizeForPage } from '@/lib/typesetting'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

describe('ProjectTypesettingDefaults', () => {
  let typesetting: Record<string, unknown> = {}
  const applied: unknown[] = []
  const puts: unknown[] = []

  beforeEach(() => {
    typesetting = {}
    applied.length = 0
    puts.length = 0
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json({ epoch: 0, scene: { pages: {}, project: { name: 'P', style: {} } } }),
      ),
      http.get('/api/v1/typesetting', () => HttpResponse.json(typesetting)),
      http.put('/api/v1/typesetting', async ({ request }) => {
        typesetting = (await request.json()) as Record<string, unknown>
        puts.push(typesetting)
        return HttpResponse.json(typesetting)
      }),
      http.post('/api/v1/history/apply', async ({ request }) => {
        applied.push(await request.json())
        return HttpResponse.json({ epoch: 1 })
      }),
    )
  })

  it('saves the selected block font and size with the page height', async () => {
    renderWithQuery(
      <ProjectTypesettingDefaults
        selectedFont='Kanit'
        selectedFontSize={28}
        pageHeight={1518}
        selectedColor={[255, 255, 255, 255]}
        hasSelection
      />,
    )

    expect(await screen.findByTestId('project-typesetting-size')).toHaveTextContent(
      'typesetting.notSetAuto',
    )
    await userEvent.click(screen.getByTestId('project-typesetting-save'))

    await waitFor(() => expect(puts).toHaveLength(1))
    expect(puts[0]).toEqual({
      fontSizePx: 28,
      referencePageHeight: 1518,
      color: [255, 255, 255, 255],
      shrinkToFit: false,
    })

    // Shrink toggle appears once a size is set and saves the flag.
    await userEvent.click(await screen.findByTestId('project-typesetting-shrink'))
    await waitFor(() => expect(puts).toHaveLength(2))
    expect(puts[1]).toMatchObject({ fontSizePx: 28, shrinkToFit: true })
    expect(screen.getByTestId('project-typesetting-color')).toHaveTextContent('#ffffff')
    expect(applied[0]).toMatchObject({
      updateProjectMeta: { patch: { style: { defaultFont: 'Kanit' } } },
    })
  })

  it('collapses and expands', async () => {
    renderWithQuery(<ProjectTypesettingDefaults hasSelection={false} pageHeight={1000} />)
    const toggle = await screen.findByTestId('project-typesetting-toggle')
    expect(screen.getByTestId('project-typesetting-save')).toBeInTheDocument()
    await userEvent.click(toggle)
    expect(screen.queryByTestId('project-typesetting-save')).toBeNull()
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    await userEvent.click(toggle)
    expect(screen.getByTestId('project-typesetting-save')).toBeInTheDocument()
  })

  it('disables saving without a selection', async () => {
    renderWithQuery(<ProjectTypesettingDefaults hasSelection={false} pageHeight={1000} />)
    expect(await screen.findByTestId('project-typesetting-save')).toBeDisabled()
  })

  it('toggles the bubble layout and keeps other settings when saving', async () => {
    typesetting = { thaiLstmLineBreaking: true, stroke: { enabled: true, widthPx: 3 } }
    renderWithQuery(
      <ProjectTypesettingDefaults selectedFontSize={30} pageHeight={1500} hasSelection />,
    )

    const bubble = await screen.findByTestId('project-typesetting-expand-bubble')
    await waitFor(() =>
      expect(screen.getByTestId('project-typesetting-thai-lstm')).toHaveAttribute(
        'aria-checked',
        'true',
      ),
    )
    await userEvent.click(bubble)
    await waitFor(() => expect(puts).toHaveLength(1))
    expect(puts[0]).toMatchObject({ thaiLstmLineBreaking: true, expandIntoBubble: true })

    await userEvent.click(screen.getByTestId('project-typesetting-save'))
    await waitFor(() => expect(puts).toHaveLength(2))
    expect(puts[1]).toMatchObject({
      fontSizePx: 30,
      thaiLstmLineBreaking: true,
      expandIntoBubble: true,
      stroke: { enabled: true, widthPx: 3 },
    })
  })
})

describe('defaultSizeForPage', () => {
  it('scales by page height and ignores unset sizes', () => {
    expect(defaultSizeForPage({}, 1000)).toBeNull()
    expect(defaultSizeForPage({ fontSizePx: 30, referencePageHeight: 1500 }, 3000)).toBe(60)
    expect(defaultSizeForPage({ fontSizePx: 24 }, 6000)).toBe(24)
  })
})
