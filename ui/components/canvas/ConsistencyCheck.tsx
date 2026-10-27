'use client'

import { CheckCheckIcon, XIcon } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { textNodesOf } from '@/hooks/useCurrentPage'
import { useScene } from '@/hooks/useScene'
import { checkConsistency, type Issue, type IssueKind } from '@/lib/consistency'
import { useGlossaryQuery } from '@/lib/glossary'
import { useSelectionStore } from '@/lib/stores/selectionStore'
import { cn } from '@/lib/utils'

const KIND_ORDER: IssueKind[] = ['glossary', 'inconsistent', 'untranslated']

const KIND_TONE: Record<IssueKind, string> = {
  glossary: 'text-amber-600',
  inconsistent: 'text-sky-600',
  untranslated: 'text-rose-600',
}

/**
 * Toolbar button + floating list of translation problems across the whole
 * project. The list stays open while the user fixes blocks and updates live
 * from the scene; clicking an entry opens its page and selects the block.
 */
export function ConsistencyCheck() {
  const { t } = useTranslation()
  const { scene } = useScene()
  const glossary = useGlossaryQuery().data?.entries ?? []
  const [open, setOpen] = useState(false)
  const currentNode = useSelectionStore((s) => s.nodeIds)

  const issues = useMemo(() => {
    if (!scene || !open) return []
    const pages = Object.entries(scene.pages ?? {}).map(([id, page]) => ({
      id,
      blocks: textNodesOf(page).map((n) => ({
        id: n.id,
        text: n.data.text,
        translation: n.data.translation,
      })),
    }))
    return checkConsistency(pages, glossary)
  }, [scene, glossary, open])

  const grouped = useMemo(
    () => KIND_ORDER.map((kind) => [kind, issues.filter((i) => i.kind === kind)] as const),
    [issues],
  )

  const jump = (issue: Issue) => {
    const sel = useSelectionStore.getState()
    if (sel.pageId !== issue.pageId) sel.setPage(issue.pageId)
    useSelectionStore.getState().select(issue.nodeId)
  }

  return (
    <Popover open={open} onOpenChange={setOpen} modal={false}>
      <PopoverTrigger asChild>
        <Button
          variant='ghost'
          size='xs'
          data-testid='toolbar-check'
          disabled={!scene}
          className={cn(open && 'bg-accent')}
        >
          <CheckCheckIcon className='size-4' />
          {t('check.button')}
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align='start'
        className='flex max-h-[70vh] w-96 flex-col gap-2 p-2'
        // Stay open while the user edits blocks on the canvas / panels.
        onInteractOutside={(e) => e.preventDefault()}
        onOpenAutoFocus={(e) => e.preventDefault()}
        data-testid='check-panel'
      >
        <div className='flex items-center justify-between gap-2'>
          <span className='text-xs font-medium'>
            {issues.length === 0
              ? t('check.allGood')
              : t('check.summary', { count: issues.length })}
          </span>
          <Button
            aria-label={t('check.close')}
            variant='ghost'
            size='icon-xs'
            onClick={() => setOpen(false)}
          >
            <XIcon className='size-3' />
          </Button>
        </div>
        <div className='min-h-0 flex-1 overflow-y-auto'>
          {grouped.map(([kind, list]) =>
            list.length === 0 ? null : (
              <div key={kind} className='mb-2'>
                <div
                  className={cn(
                    'sticky top-0 bg-popover py-0.5 text-[10px] font-semibold uppercase',
                    KIND_TONE[kind],
                  )}
                >
                  {t(`check.kind.${kind}`)} ({list.length})
                </div>
                <div className='flex flex-col'>
                  {list.map((issue, i) => {
                    const active = currentNode.has(issue.nodeId)
                    return (
                      <button
                        key={`${issue.nodeId}-${i}`}
                        type='button'
                        data-testid={`check-issue-${kind}`}
                        onClick={() => jump(issue)}
                        className={cn(
                          'flex flex-col gap-0.5 rounded px-1.5 py-1 text-left text-[11px] hover:bg-accent',
                          active && 'bg-accent',
                        )}
                      >
                        <span className='flex items-baseline gap-1.5'>
                          <span className='shrink-0 font-mono text-[10px] text-muted-foreground'>
                            {t('check.where', {
                              page: issue.pageNumber,
                              block: issue.blockNumber,
                            })}
                          </span>
                          <span className='truncate'>{issue.source}</span>
                        </span>
                        {issue.translation && (
                          <span className='truncate pl-1 text-muted-foreground'>
                            → {issue.translation}
                          </span>
                        )}
                        {issue.detail && (
                          <span className={cn('truncate pl-1 text-[10px]', KIND_TONE[kind])}>
                            {kind === 'glossary'
                              ? t('check.expected', { term: issue.detail })
                              : t('check.elsewhere', { other: issue.detail })}
                          </span>
                        )}
                      </button>
                    )
                  })}
                </div>
              </div>
            ),
          )}
        </div>
      </PopoverContent>
    </Popover>
  )
}
