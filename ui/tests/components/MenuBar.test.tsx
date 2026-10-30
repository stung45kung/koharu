import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { MenuBar } from '@/components/MenuBar'
import { getGetConfigQueryKey, getGetSceneJsonQueryKey } from '@/lib/api/default/default'
import { queryClient } from '@/lib/queryClient'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'

vi.mock('@/lib/io/openFiles', () => ({
  openImageFiles: vi.fn().mockResolvedValue([]),
  openImageFolder: vi.fn().mockResolvedValue([]),
  openKhrFile: vi.fn().mockResolvedValue(null),
}))

beforeEach(() => {
  // Default: config + scene exist so the menu enables scene-dependent items.
  server.use(
    http.get('/api/v1/scene.json', () =>
      HttpResponse.json({
        epoch: 0,
        scene: { pages: {}, project: { name: 'P' } as never },
      }),
    ),
    http.get('/api/v1/config', () => HttpResponse.json({})),
  )
  queryClient.setQueryData(getGetSceneJsonQueryKey(), {
    epoch: 0,
    scene: { pages: {}, project: { name: 'P' } },
  })
  queryClient.setQueryData(getGetConfigQueryKey(), {})
})

describe('MenuBar', () => {
  it('renders File / View / Process / Help triggers', async () => {
    renderWithQuery(<MenuBar />)
    expect(screen.getByTestId('menu-file-trigger')).toBeInTheDocument()
    expect(screen.getByTestId('menu-process-trigger')).toBeInTheDocument()
  })

  it('Close Project calls DELETE /projects/current and invalidates scene', async () => {
    let deleted = 0
    server.use(
      http.delete('/api/v1/projects/current', () => {
        deleted += 1
        return new HttpResponse(null, { status: 204 })
      }),
    )
    const invalidateSpy = vi.spyOn(queryClient, 'invalidateQueries')

    renderWithQuery(<MenuBar />)
    await userEvent.click(screen.getByTestId('menu-file-trigger'))
    const close = await screen.findByTestId('menu-file-close-project')
    await userEvent.click(close)

    await waitFor(() => expect(deleted).toBe(1))
    await waitFor(() => {
      const invalidatedKeys = invalidateSpy.mock.calls.map((c) => c[0]?.queryKey)
      expect(invalidatedKeys).toContainEqual(getGetSceneJsonQueryKey())
    })
  })

  it('Close Project is disabled when no project is open', async () => {
    // Clear seeded cache + point /scene.json at the 400 response so useScene
    // resolves to null.
    queryClient.clear()
    server.use(
      http.get('/api/v1/scene.json', () =>
        HttpResponse.json({ message: 'no project' }, { status: 400 }),
      ),
    )
    renderWithQuery(<MenuBar />)
    await waitFor(() => expect(queryClient.isFetching()).toBe(0))
    await userEvent.click(screen.getByTestId('menu-file-trigger'))
    const close = await screen.findByTestId('menu-file-close-project')
    expect(close).toHaveAttribute('data-disabled')
  })

  it('adds the ocr-check step after OCR when the switch is on', async () => {
    const pipelineRequests: any[] = []
    server.use(
      http.get('/api/v1/config', () =>
        HttpResponse.json({ pipeline: { ocr: 'manga-ocr', translator: 'llm' } }),
      ),
      http.post('/api/v1/pipelines', async ({ request }) => {
        pipelineRequests.push(await request.json())
        return HttpResponse.json({ operationId: 'op-1' })
      }),
    )
    usePreferencesStore.setState({ ocrCheck: false })
    useSelectionStore.getState().setPage('p1')

    renderWithQuery(<MenuBar />)
    await userEvent.click(screen.getByTestId('menu-process-trigger'))
    await userEvent.click(await screen.findByTestId('menu-ocr-check'))
    expect(usePreferencesStore.getState().ocrCheck).toBe(true)
    await userEvent.click(await screen.findByTestId('menu-process-current'))

    await waitFor(() => expect(pipelineRequests).toHaveLength(1))
    const steps: string[] = pipelineRequests[0].steps
    expect(steps.slice(steps.indexOf('manga-ocr'), steps.indexOf('manga-ocr') + 2)).toEqual([
      'manga-ocr',
      'ocr-check',
    ])
  })

  it('leaves the ocr-check step out when the switch is off', async () => {
    const pipelineRequests: any[] = []
    server.use(
      http.get('/api/v1/config', () =>
        HttpResponse.json({ pipeline: { ocr: 'manga-ocr', translator: 'llm' } }),
      ),
      http.post('/api/v1/pipelines', async ({ request }) => {
        pipelineRequests.push(await request.json())
        return HttpResponse.json({ operationId: 'op-1' })
      }),
    )
    usePreferencesStore.setState({ ocrCheck: false })
    useSelectionStore.getState().setPage('p1')

    renderWithQuery(<MenuBar />)
    await userEvent.click(screen.getByTestId('menu-process-trigger'))
    await userEvent.click(await screen.findByTestId('menu-process-current'))

    await waitFor(() => expect(pipelineRequests).toHaveLength(1))
    expect(pipelineRequests[0].steps).not.toContain('ocr-check')
  })
})
