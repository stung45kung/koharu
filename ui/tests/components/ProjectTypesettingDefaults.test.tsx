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
        hasSelection
      />,
    )

    expect(await screen.findByTestId('project-typesetting-size')).toHaveTextContent(
      'typesetting.notSetAuto',
    )
    await userEvent.click(screen.getByTestId('project-typesetting-save'))

    await waitFor(() => expect(puts).toHaveLength(1))
    expect(puts[0]).toEqual({ fontSizePx: 28, referencePageHeight: 1518 })
    expect(applied[0]).toMatchObject({
      updateProjectMeta: { patch: { style: { defaultFont: 'Kanit' } } },
    })
  })

  it('disables saving without a selection', async () => {
    renderWithQuery(<ProjectTypesettingDefaults hasSelection={false} pageHeight={1000} />)
    expect(await screen.findByTestId('project-typesetting-save')).toBeDisabled()
  })
})

describe('defaultSizeForPage', () => {
  it('scales by page height and ignores unset sizes', () => {
    expect(defaultSizeForPage({}, 1000)).toBeNull()
    expect(defaultSizeForPage({ fontSizePx: 30, referencePageHeight: 1500 }, 3000)).toBe(60)
    expect(defaultSizeForPage({ fontSizePx: 24 }, 6000)).toBe(24)
  })
})
