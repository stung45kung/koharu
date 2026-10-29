'use client'

import { Loader2Icon, PlusIcon, Trash2Icon, WandSparklesIcon, XIcon } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { suggestTerms, useGlossaryQuery, useSaveGlossary, type GlossaryEntry } from '@/lib/glossary'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
}

type Row = GlossaryEntry & { key: number }
type Suggestion = GlossaryEntry & { key: number; checked: boolean }

let nextKey = 0
const withKey = (e: GlossaryEntry): Row => ({ ...e, key: nextKey++ })

/**
 * Edit the project glossary: source term → required translation (+ note).
 * Kept as a local draft and written on Save.
 */
export function GlossaryDialog({ open, onOpenChange }: Props) {
  const { t } = useTranslation()
  const query = useGlossaryQuery()
  const save = useSaveGlossary()
  const [rows, setRows] = useState<Row[]>([])
  const [initialized, setInitialized] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [suggestions, setSuggestions] = useState<Suggestion[] | null>(null)
  const [suggestInfo, setSuggestInfo] = useState<string | null>(null)
  const [suggesting, setSuggesting] = useState(false)

  // Seed once per opening, only from loaded data (never save an empty draft
  // over an existing glossary).
  useEffect(() => {
    if (!open) {
      setInitialized(false)
      setSuggestions(null)
      setSuggestInfo(null)
      return
    }
    if (!initialized && query.isSuccess) {
      setRows(query.data.entries.map(withKey))
      setError(null)
      setInitialized(true)
    }
  }, [open, initialized, query.isSuccess, query.data])

  const runSuggest = async () => {
    setSuggesting(true)
    setError(null)
    setSuggestInfo(null)
    try {
      const res = await suggestTerms(useEditorUiStore.getState().selectedLanguage)
      const have = new Set(rows.map((r) => r.source.trim().toLowerCase()))
      const fresh = res.suggestions.filter((s) => !have.has(s.source.trim().toLowerCase()))
      setSuggestions(fresh.map((s) => ({ ...s, key: nextKey++, checked: true })))
      setSuggestInfo(
        t('glossary.suggestFound', { count: fresh.length, lines: res.linesScanned }) +
          (res.truncated ? ` ${t('glossary.suggestTruncated')}` : ''),
      )
    } catch (err) {
      setError(String(err))
    } finally {
      setSuggesting(false)
    }
  }

  const updateSuggestion = (key: number, patch: Partial<Suggestion>) =>
    setSuggestions((prev) => prev?.map((s) => (s.key === key ? { ...s, ...patch } : s)) ?? null)

  const addSuggestions = () => {
    const picked = (suggestions ?? []).filter((s) => s.checked && s.source.trim())
    setRows((prev) => [
      ...prev,
      ...picked.map(({ source, target, note }) => withKey({ source, target, note })),
    ])
    setSuggestions(null)
    setSuggestInfo(t('glossary.suggestAdded', { count: picked.length }))
  }

  const update = (key: number, patch: Partial<GlossaryEntry>) =>
    setRows((prev) => prev.map((r) => (r.key === key ? { ...r, ...patch } : r)))

  const handleSave = async () => {
    try {
      await save.mutateAsync({
        entries: rows
          .filter((r) => r.source.trim())
          .map(({ source, target, note }) => ({ source, target, note })),
      })
      onOpenChange(false)
    } catch (err) {
      setError(String(err))
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className='flex h-[80vh] w-[95vw] max-w-4xl flex-col gap-3 sm:max-w-4xl'
        data-testid='glossary-dialog'
      >
        <div className='shrink-0 pr-6'>
          <DialogTitle>{t('glossary.title')}</DialogTitle>
          <DialogDescription className='mt-1 text-xs'>{t('glossary.description')}</DialogDescription>
        </div>

        <div className='grid shrink-0 grid-cols-[1fr_1fr_1.4fr_auto] gap-2 px-1 text-[10px] font-medium text-muted-foreground uppercase'>
          <span>{t('glossary.source')}</span>
          <span>{t('glossary.target')}</span>
          <span>{t('glossary.note')}</span>
          <span className='w-7' />
        </div>
        {suggestions && (
          <div
            className='flex max-h-[45%] shrink-0 flex-col gap-1.5 rounded-md p-2 ring-1 ring-sky-500/40'
            data-testid='glossary-suggestions'
          >
            <div className='flex items-center justify-between gap-2'>
              <span className='text-xs font-medium'>{t('glossary.suggestTitle')}</span>
              <Button
                aria-label={t('glossary.suggestDismiss')}
                variant='ghost'
                size='icon-xs'
                onClick={() => setSuggestions(null)}
              >
                <XIcon className='size-3' />
              </Button>
            </div>
            {suggestions.length === 0 ? (
              <p className='text-xs text-muted-foreground'>{t('glossary.suggestNone')}</p>
            ) : (
              <div className='min-h-0 flex-1 overflow-y-auto'>
                <div className='flex flex-col gap-1'>
                  {suggestions.map((s) => (
                    <div
                      key={s.key}
                      className='grid grid-cols-[auto_1fr_1fr_1.4fr] items-center gap-2 px-1'
                    >
                      <input
                        type='checkbox'
                        className='size-3.5 accent-primary'
                        checked={s.checked}
                        onChange={(e) => updateSuggestion(s.key, { checked: e.target.checked })}
                      />
                      <Input
                        value={s.source}
                        maxLength={200}
                        onChange={(e) => updateSuggestion(s.key, { source: e.target.value })}
                        className='h-7 text-xs'
                      />
                      <Input
                        value={s.target}
                        maxLength={200}
                        onChange={(e) => updateSuggestion(s.key, { target: e.target.value })}
                        className='h-7 text-xs'
                      />
                      <Input
                        value={s.note}
                        maxLength={200}
                        onChange={(e) => updateSuggestion(s.key, { note: e.target.value })}
                        className='h-7 text-xs'
                      />
                    </div>
                  ))}
                </div>
              </div>
            )}
            {suggestions.length > 0 && (
              <div className='flex items-center justify-end gap-2'>
                <Button
                  variant='ghost'
                  size='xs'
                  onClick={() =>
                    setSuggestions((prev) => {
                      const all = prev?.every((s) => s.checked) ?? false
                      return prev?.map((s) => ({ ...s, checked: !all })) ?? null
                    })
                  }
                >
                  {t('glossary.suggestToggleAll')}
                </Button>
                <Button
                  size='xs'
                  data-testid='glossary-suggest-add'
                  disabled={!suggestions.some((s) => s.checked)}
                  onClick={addSuggestions}
                >
                  {t('glossary.suggestAdd', {
                    count: suggestions.filter((s) => s.checked).length,
                  })}
                </Button>
              </div>
            )}
          </div>
        )}
        <div className='min-h-0 flex-1 overflow-y-auto' data-testid='glossary-rows'>
          {rows.length === 0 && (
            <p className='rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground'>
              {t('glossary.empty')}
            </p>
          )}
          <div className='flex flex-col gap-1.5'>
            {rows.map((r, index) => (
              <div
                key={r.key}
                className='grid grid-cols-[1fr_1fr_1.4fr_auto] items-center gap-2 px-1'
                data-testid={`glossary-row-${index}`}
              >
                <Input
                  data-testid={`glossary-source-${index}`}
                  value={r.source}
                  maxLength={200}
                  placeholder='すい'
                  onChange={(e) => update(r.key, { source: e.target.value })}
                  className='h-7 text-xs'
                />
                <Input
                  data-testid={`glossary-target-${index}`}
                  value={r.target}
                  maxLength={200}
                  placeholder='ซุย'
                  onChange={(e) => update(r.key, { target: e.target.value })}
                  className='h-7 text-xs'
                />
                <Input
                  data-testid={`glossary-note-${index}`}
                  value={r.note}
                  maxLength={200}
                  placeholder={t('glossary.notePlaceholder')}
                  onChange={(e) => update(r.key, { note: e.target.value })}
                  className='h-7 text-xs'
                />
                <Button
                  aria-label={t('glossary.delete')}
                  data-testid={`glossary-delete-${index}`}
                  variant='ghost'
                  size='icon-xs'
                  onClick={() => setRows((prev) => prev.filter((x) => x.key !== r.key))}
                  className='size-7 text-rose-600 hover:text-rose-600'
                >
                  <Trash2Icon className='size-3.5' />
                </Button>
              </div>
            ))}
          </div>
        </div>

        <div className='flex shrink-0 items-center justify-end gap-2'>
          <Button
            variant='outline'
            size='sm'
            disabled={!initialized}
            data-testid='glossary-add'
            onClick={() => setRows((prev) => [...prev, withKey({ source: '', target: '', note: '' })])}
          >
            <PlusIcon className='size-3.5' />
            {t('glossary.add')}
          </Button>
          <Button
            variant='outline'
            size='sm'
            disabled={!initialized || suggesting}
            data-testid='glossary-suggest'
            onClick={() => void runSuggest()}
          >
            {suggesting ? (
              <Loader2Icon className='size-3.5 animate-spin' />
            ) : (
              <WandSparklesIcon className='size-3.5' />
            )}
            {t('glossary.suggest')}
          </Button>
          <span className='mr-auto' />
          {suggestInfo && !error && (
            <p className='text-xs text-muted-foreground'>{suggestInfo}</p>
          )}
          {error && <p className='text-xs text-rose-600'>{error}</p>}
          <Button variant='ghost' size='sm' onClick={() => onOpenChange(false)}>
            {t('characters.cancel')}
          </Button>
          <Button
            size='sm'
            disabled={!initialized || save.isPending}
            data-testid='glossary-save'
            onClick={() => void handleSave()}
          >
            {save.isPending && <Loader2Icon className='size-3.5 animate-spin' />}
            {t('characters.save')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
