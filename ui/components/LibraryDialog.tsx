'use client'

import { DownloadIcon, Loader2Icon, SaveIcon, Trash2Icon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useScene } from '@/hooks/useScene'
import {
  useApplyPreset,
  useDeletePreset,
  useLibrary,
  useSavePreset,
  type MergeOutcome,
} from '@/lib/library'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * Series library: save this project's characters + glossary (+ typesetting
 * defaults) under a name, and load a saved set into another project.
 */
export function LibraryDialog({ open, onOpenChange }: Props) {
  const { t } = useTranslation()
  const { scene } = useScene()
  const library = useLibrary(open)
  const save = useSavePreset()
  const apply = useApplyPreset()
  const remove = useDeletePreset()
  const [name, setName] = useState('')
  const [applyTypesetting, setApplyTypesetting] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null)

  useEffect(() => {
    if (open) {
      setName((prev) => prev || scene?.project?.name || '')
      setMessage(null)
      setError(null)
      setConfirmDelete(null)
    }
  }, [open, scene?.project?.name])

  const presets = library.data ?? []
  const exists = presets.some((p) => p.name.toLowerCase() === name.trim().toLowerCase())

  const run = async (fn: () => Promise<string>) => {
    setError(null)
    setMessage(null)
    try {
      setMessage(await fn())
    } catch (err) {
      setError(String(err))
    }
  }

  const describe = (o: MergeOutcome) =>
    t('library.applied', {
      added: o.charactersAdded,
      updated: o.charactersUpdated,
      gAdded: o.glossaryAdded,
      gUpdated: o.glossaryUpdated,
    }) + (o.typesettingApplied ? ` ${t('library.typesettingApplied')}` : '')

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className='flex max-h-[85vh] w-[95vw] max-w-2xl flex-col gap-3 sm:max-w-2xl'>
        <div className='shrink-0 pr-6'>
          <DialogTitle>{t('library.title')}</DialogTitle>
          <DialogDescription className='mt-1 text-xs'>{t('library.description')}</DialogDescription>
        </div>

        {/* Save current project */}
        <div className='flex shrink-0 flex-col gap-1.5 rounded-md p-2 ring-1 ring-border'>
          <span className='text-xs font-medium'>{t('library.saveTitle')}</span>
          <div className='flex items-center gap-2'>
            <Input
              data-testid='library-name'
              value={name}
              maxLength={80}
              placeholder={t('library.namePlaceholder')}
              onChange={(e) => setName(e.target.value)}
              className='h-8 text-sm'
            />
            <Button
              size='sm'
              data-testid='library-save'
              disabled={!name.trim() || save.isPending}
              onClick={() =>
                void run(async () => {
                  const saved = await save.mutateAsync(name.trim())
                  return t('library.saved', {
                    name: saved.name,
                    characters: saved.characters,
                    glossary: saved.glossaryEntries,
                  })
                })
              }
            >
              {save.isPending ? (
                <Loader2Icon className='size-3.5 animate-spin' />
              ) : (
                <SaveIcon className='size-3.5' />
              )}
              {exists ? t('library.overwrite') : t('library.save')}
            </Button>
          </div>
          <p className='text-[10px] text-muted-foreground'>{t('library.saveHint')}</p>
        </div>

        {/* Saved presets */}
        <div className='flex min-h-0 flex-1 flex-col gap-1.5'>
          <div className='flex items-center justify-between gap-2'>
            <span className='text-xs font-medium'>{t('library.listTitle')}</span>
            <label className='flex items-center gap-1.5 text-[11px] text-muted-foreground'>
              <Switch
                size='sm'
                data-testid='library-apply-typesetting'
                checked={applyTypesetting}
                onCheckedChange={setApplyTypesetting}
              />
              {t('library.applyTypesetting')}
            </label>
          </div>
          <div
            className='min-h-24 flex-1 overflow-y-auto rounded-md p-1 ring-1 ring-border'
            data-testid='library-list'
          >
            {library.isLoading && (
              <p className='p-2 text-xs text-muted-foreground'>{t('library.loading')}</p>
            )}
            {!library.isLoading && presets.length === 0 && (
              <p className='p-2 text-xs text-muted-foreground'>{t('library.empty')}</p>
            )}
            {presets.map((p) => (
              <div
                key={p.name}
                className='flex items-center gap-2 rounded px-2 py-1.5 text-xs hover:bg-accent'
                data-testid='library-item'
              >
                <button
                  type='button'
                  className='min-w-0 flex-1 text-left'
                  onClick={() => setName(p.name)}
                >
                  <div className='truncate font-medium'>{p.name}</div>
                  <div className='text-[10px] text-muted-foreground'>
                    {t('library.counts', {
                      characters: p.characters,
                      glossary: p.glossaryEntries,
                    })}
                    {p.hasTypesetting ? ` · ${t('library.hasTypesetting')}` : ''}
                    {p.updatedAtMs ? ` · ${new Date(p.updatedAtMs).toLocaleDateString()}` : ''}
                  </div>
                </button>
                <Button
                  variant='outline'
                  size='xs'
                  className='text-[11px]'
                  data-testid='library-apply'
                  disabled={!scene || apply.isPending}
                  onClick={() =>
                    void run(async () => {
                      const res = await apply.mutateAsync({ name: p.name, applyTypesetting })
                      return describe(res.outcome)
                    })
                  }
                >
                  <DownloadIcon className='size-3' />
                  {t('library.apply')}
                </Button>
                {confirmDelete === p.name ? (
                  <Button
                    variant='destructive'
                    size='xs'
                    className='text-[11px]'
                    data-testid='library-delete-confirm'
                    onClick={() =>
                      void run(async () => {
                        await remove.mutateAsync(p.name)
                        setConfirmDelete(null)
                        return t('library.deleted', { name: p.name })
                      })
                    }
                  >
                    {t('library.confirmDelete')}
                  </Button>
                ) : (
                  <Button
                    aria-label={t('library.delete')}
                    variant='ghost'
                    size='icon-xs'
                    className='text-rose-600 hover:text-rose-600'
                    data-testid='library-delete'
                    onClick={() => setConfirmDelete(p.name)}
                  >
                    <Trash2Icon className='size-3' />
                  </Button>
                )}
              </div>
            ))}
          </div>
          <p className='text-[10px] text-muted-foreground'>{t('library.applyHint')}</p>
        </div>

        <div className='flex shrink-0 items-center justify-end gap-2'>
          {message && <p className='mr-auto text-xs text-emerald-600'>{message}</p>}
          {error && <p className='mr-auto text-xs text-rose-600'>{error}</p>}
          <Button variant='ghost' size='sm' onClick={() => onOpenChange(false)}>
            {t('library.close')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
