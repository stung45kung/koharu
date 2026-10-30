import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'

import { TextBlocksPanel } from '@/components/panels/TextBlocksPanel'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useJobsStore } from '@/lib/stores/jobsStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

import { renderWithQuery } from '../helpers'
import { server } from '../msw/server'
import { queryClient } from '@/lib/queryClient'

function sceneWithTextNodes() {
  return {
    epoch: 1,
    scene: {
      pages: {
        p1: {
          id: 'p1',
          name: 'P1',
          width: 100,
          height: 100,
          nodes: {
            t1: {
              id: 't1',
              transform: { x: 0, y: 0, width: 10, height: 10, rotationDeg: 0 },
              visible: true,
              kind: { text: { text: 'first' } },
            },
            t2: {
              id: 't2',
              transform: { x: 10, y: 10, width: 10, height: 10, rotationDeg: 0 },
              visible: true,
              kind: { text: { text: 'second' } },
            },
          },
        },
      },
      project: { name: 'Proj' },
    },
  }
}

describe('TextBlocksPanel', () => {
  beforeEach(() => {
    useSelectionStore.getState().setPage('p1')
    useSelectionStore.getState().select('t2', false)
    useJobsStore.getState().clear()
    queryClient.clear()
    useEditorUiStore.setState({ selectedLanguage: 'en' })
    usePreferencesStore.setState({
      customSystemPrompt: 'translate naturally',
      defaultFont: 'Arial',
    })
  })

  it('generates translation only for the clicked text block', async () => {
    const pipelineRequests: unknown[] = []
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.get('/api/v1/config', () =>
        HttpResponse.json({ pipeline: { translator: 'llm', renderer: 'koharu-renderer' } }),
      ),
      http.get('/api/v1/llm/current', () =>
        HttpResponse.json({ status: 'ready', target: null, error: null }),
      ),
      http.post('/api/v1/pipelines', async ({ request }) => {
        pipelineRequests.push(await request.json())
        return HttpResponse.json({ operationId: 'op-1' })
      }),
    )

    renderWithQuery(<TextBlocksPanel />)

    const generateButton = await screen.findByTestId('textblock-generate-1')
    await waitFor(() => expect(generateButton).not.toBeDisabled())
    await userEvent.click(generateButton)

    await waitFor(() => expect(pipelineRequests).toHaveLength(1))
    expect(pipelineRequests[0]).toMatchObject({
      steps: ['llm', 'koharu-renderer'],
      pages: ['p1'],
      textNodeIds: ['t2'],
      targetLanguage: 'en',
      systemPrompt: 'translate naturally',
      defaultFont: 'Arial',
    })
  })

  it('reads only the clicked block again with the configured OCR', async () => {
    const pipelineRequests: unknown[] = []
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.get('/api/v1/config', () =>
        HttpResponse.json({ pipeline: { ocr: 'paddle-ocr-vl-1.6' } }),
      ),
      http.post('/api/v1/pipelines', async ({ request }) => {
        pipelineRequests.push(await request.json())
        return HttpResponse.json({ operationId: 'op-1' })
      }),
    )

    renderWithQuery(<TextBlocksPanel />)

    await userEvent.click(await screen.findByTestId('textblock-reocr-1'))

    await waitFor(() => expect(pipelineRequests).toHaveLength(1))
    expect(pipelineRequests[0]).toMatchObject({
      steps: ['paddle-ocr-vl-1.6'],
      pages: ['p1'],
      textNodeIds: ['t2'],
    })
  })

  it('shows the assigned speaker on the block header', async () => {
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.get('/api/v1/llm/current', () =>
        HttpResponse.json({ status: 'ready', target: null, error: null }),
      ),
      http.get('/api/v1/characters', () =>
        HttpResponse.json({
          version: 1,
          characters: [{ id: 'c1', name: 'Narrator', description: '' }],
          speakers: { t1: 'c1' },
        }),
      ),
    )

    renderWithQuery(<TextBlocksPanel />)

    const chip = await screen.findByTestId('textblock-speaker-chip-0')
    expect(chip).toHaveTextContent('Narrator')
    expect(screen.queryByTestId('textblock-speaker-chip-1')).toBeNull()
  })

  it('opens the character manager from the panel header', async () => {
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.get('/api/v1/llm/current', () =>
        HttpResponse.json({ status: 'ready', target: null, error: null }),
      ),
    )

    renderWithQuery(<TextBlocksPanel />)

    await userEvent.click(await screen.findByTestId('textblocks-characters'))
    expect(await screen.findByTestId('characters-dialog')).toBeInTheDocument()
  })

  it('guesses speakers for the page and marks them as suggested', async () => {
    const guessRequests: unknown[] = []
    let book = {
      version: 1,
      characters: [{ id: 'c1', name: 'Captain', description: '' }],
      speakers: {} as Record<string, string>,
      suggested: [] as string[],
    }
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.get('/api/v1/llm/current', () =>
        HttpResponse.json({ status: 'ready', target: null, error: null }),
      ),
      http.get('/api/v1/characters', () => HttpResponse.json(book)),
      http.post('/api/v1/characters/guess', async ({ request }) => {
        guessRequests.push(await request.json())
        book = { ...book, speakers: { t1: 'c1' }, suggested: ['t1'] }
        return HttpResponse.json({ book, considered: 2, assigned: 1 })
      }),
      http.post('/api/v1/characters/confirm', async ({ request }) => {
        const body = (await request.json()) as { nodeIds: string[] }
        book = { ...book, suggested: book.suggested.filter((n) => !body.nodeIds.includes(n)) }
        return HttpResponse.json(book)
      }),
    )

    renderWithQuery(<TextBlocksPanel />)

    const guessButton = await screen.findByTestId('textblocks-guess-speakers')
    await waitFor(() => expect(guessButton).not.toBeDisabled())
    await userEvent.click(guessButton)

    await waitFor(() => expect(guessRequests).toHaveLength(1))
    expect(guessRequests[0]).toMatchObject({ pageId: 'p1', targetLanguage: 'en' })

    const chip = await screen.findByTestId('textblock-speaker-chip-0')
    expect(chip).toHaveAttribute('data-suggested', 'true')
    expect(chip).toHaveTextContent('Captain?')

    await userEvent.click(screen.getByTestId('textblocks-confirm-speakers'))
    await waitFor(() =>
      expect(screen.getByTestId('textblock-speaker-chip-0')).toHaveAttribute(
        'data-suggested',
        'false',
      ),
    )
    expect(screen.queryByTestId('textblocks-confirm-speakers')).toBeNull()
  })

  it('disables guessing until characters exist', async () => {
    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.get('/api/v1/llm/current', () =>
        HttpResponse.json({ status: 'ready', target: null, error: null }),
      ),
    )

    renderWithQuery(<TextBlocksPanel />)

    expect(await screen.findByTestId('textblocks-guess-speakers')).toBeDisabled()
    expect(screen.getByTestId('textblocks-guess-hint')).toHaveTextContent(
      'characters.guessNeedsCharacters',
    )
  })

  it('deletes the corresponding text block when the delete button is clicked', async () => {
    let lastOp: any = null

    server.use(
      http.get('/api/v1/scene.json', () => HttpResponse.json(sceneWithTextNodes())),
      http.post('/api/v1/history/apply', async ({ request }) => {
        lastOp = await request.json()
        return HttpResponse.json({ epoch: 2 })
      }),
    )
    renderWithQuery(<TextBlocksPanel />)

    const block0 = await screen.findByTestId('textblock-trigger-0')
    await userEvent.click(block0)
    const button0 = await screen.findByTestId('textblock-delete-0')
    await userEvent.click(button0)

    expect(lastOp).toMatchObject({ removeNode: { id: 't1' } })

    const block1 = await screen.findByTestId('textblock-trigger-1')
    await userEvent.click(block1)
    const button1 = await screen.findByTestId('textblock-delete-1')
    await userEvent.click(button1)

    expect(lastOp).toMatchObject({ removeNode: { id: 't2' } })
  })
})
