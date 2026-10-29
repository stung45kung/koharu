'use client'

import { LoaderCircleIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useExportProgressStore } from '@/lib/io/exportProgress'

/** Bottom-right card while pages are exported one by one. */
export function ExportProgress() {
  const { t } = useTranslation()
  const progress = useExportProgressStore((s) => s.progress)
  const cancelled = useExportProgressStore((s) => s.cancelled)
  if (!progress) return null
  return (
    <div
      data-testid='export-progress'
      className='fixed right-4 bottom-10 z-50 flex items-center gap-3 rounded-md border border-border bg-card px-3 py-2 text-xs shadow-lg'
    >
      <LoaderCircleIcon className='size-4 animate-spin' />
      <span className='tabular-nums'>
        {t('export.progress', { done: progress.done, total: progress.total })}
      </span>
      <Button
        size='xs'
        variant='outline'
        disabled={cancelled}
        onClick={() => useExportProgressStore.getState().cancel()}
      >
        {t('export.cancel')}
      </Button>
    </div>
  )
}
