'use client'

import {
  AlignCenterIcon,
  AlignLeftIcon,
  AlignRightIcon,
  BoldIcon,
  CopyIcon,
  ItalicIcon,
  Loader2Icon,
  PlusIcon,
  Trash2Icon,
  XIcon,
} from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ColorPicker } from '@/components/ui/color-picker'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { useListFonts } from '@/lib/api/default/default'
import {
  EMPTY_CHARACTER_BOOK,
  newCharacterId,
  useCharactersQuery,
  useSaveCharacters,
  type Character,
} from '@/lib/characters'
import { useTypesetting } from '@/lib/typesetting'

const MAX_NAME = 80
const MAX_DESCRIPTION = 4000

type CharacterManagerDialogProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Style of the currently selected text block, for "copy from block". */
  selectedStyle?: {
    font?: string | null
    color?: number[] | null
    /** Block size in px on its page. */
    fontSize?: number | null
    bold?: boolean | null
    italic?: boolean | null
    textAlign?: 'left' | 'center' | 'right' | null
    pageHeight?: number | null
  }
}

type Align = 'left' | 'center' | 'right'

const rgbaToHex = (c: number[]) =>
  '#' +
  c
    .slice(0, 3)
    .map((v) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0'))
    .join('')

const hexToRgba = (hex: string): number[] => {
  const h = hex.replace('#', '')
  const full = h.length === 3 ? h.replace(/./g, (x) => x + x) : h.padEnd(6, '0')
  return [0, 2, 4].map((i) => Number.parseInt(full.slice(i, i + 2), 16) || 0).concat(255)
}

/**
 * Edit the open project's character list (name + free-form speech notes).
 *
 * Two panes: the character list on the left, the selected character's
 * editor on the right (its description box fills the dialog height, so long
 * profiles stay readable however many characters there are).
 *
 * Changes are kept in a local draft and written on Save; deleting a
 * character that is in use turns its blocks back into "unknown speaker".
 */
export function CharacterManagerDialog({
  open,
  onOpenChange,
  selectedStyle,
}: CharacterManagerDialogProps) {
  const { t } = useTranslation()
  const query = useCharactersQuery()
  const book = query.data ?? EMPTY_CHARACTER_BOOK
  const save = useSaveCharacters()
  const { data: fonts = [] } = useListFonts()
  const referencePageHeight = useTypesetting().referencePageHeight ?? null
  const [draft, setDraft] = useState<Character[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [initialized, setInitialized] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Seed the draft once per opening, and only from loaded data: saving a
  // draft seeded before the fetch finished would wipe existing characters.
  useEffect(() => {
    if (!open) {
      setInitialized(false)
      return
    }
    if (!initialized && query.isSuccess) {
      const seeded = query.data.characters.map((c) => ({ ...c }))
      setDraft(seeded)
      setSelectedId(seeded[0]?.id ?? null)
      setError(null)
      setInitialized(true)
    }
  }, [open, initialized, query.isSuccess, query.data])

  const usage = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const id of Object.values(book.speakers)) counts[id] = (counts[id] ?? 0) + 1
    return counts
  }, [book.speakers])

  const selected = draft.find((c) => c.id === selectedId) ?? null

  const update = (id: string, patch: Partial<Character>) =>
    setDraft((prev) => prev.map((c) => (c.id === id ? { ...c, ...patch } : c)))

  const add = () => {
    const id = newCharacterId()
    setDraft((prev) => [...prev, { id, name: '', description: '' }])
    setSelectedId(id)
  }

  const remove = (id: string) => {
    const index = draft.findIndex((c) => c.id === id)
    const next = draft.filter((c) => c.id !== id)
    setDraft(next)
    // Keep a neighbour selected so the editor doesn't go blank.
    setSelectedId(next[Math.min(index, next.length - 1)]?.id ?? null)
  }

  const handleSave = async () => {
    const blank = draft.find((c) => !c.name.trim())
    if (blank) {
      setSelectedId(blank.id)
      setError(t('characters.nameRequired'))
      return
    }
    try {
      await save.mutateAsync(draft)
      onOpenChange(false)
    } catch (err) {
      setError(String(err))
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className='flex h-[85vh] w-[95vw] max-w-5xl flex-col gap-3 sm:max-w-5xl'
        data-testid='characters-dialog'
      >
        <div className='shrink-0 pr-6'>
          <DialogTitle>{t('characters.title')}</DialogTitle>
          <DialogDescription className='mt-1 text-xs'>
            {t('characters.description')}
          </DialogDescription>
        </div>

        <div className='flex min-h-0 flex-1 gap-3'>
          {/* Character list */}
          <div className='flex w-60 shrink-0 flex-col gap-2'>
            <div
              className='min-h-0 flex-1 overflow-y-auto rounded-md p-1 ring-1 ring-border'
              data-testid='character-list'
            >
              {draft.length === 0 && (
                <p className='p-2 text-xs text-muted-foreground'>{t('characters.empty')}</p>
              )}
              {draft.map((c, index) => {
                const active = c.id === selectedId
                return (
                  <button
                    key={c.id}
                    type='button'
                    data-testid={`character-item-${index}`}
                    data-selected={active}
                    onClick={() => setSelectedId(c.id)}
                    className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs transition-colors ${
                      active ? 'bg-primary text-primary-foreground' : 'hover:bg-accent'
                    }`}
                  >
                    <span
                      className={`min-w-0 flex-1 truncate ${c.name.trim() ? '' : 'italic opacity-60'}`}
                    >
                      {c.name.trim() || t('characters.unnamed')}
                    </span>
                    {usage[c.id] ? (
                      <span className='shrink-0 text-[10px] opacity-70'>
                        {t('characters.usedBy', { count: usage[c.id] })}
                      </span>
                    ) : null}
                  </button>
                )
              })}
            </div>
            <Button
              variant='outline'
              size='sm'
              onClick={add}
              disabled={!initialized}
              data-testid='character-add'
            >
              <PlusIcon className='size-3.5' />
              {t('characters.add')}
            </Button>
          </div>

          {/* Editor for the selected character */}
          <div className='flex min-w-0 flex-1 flex-col gap-2'>
            {selected ? (
              <>
                <div className='flex shrink-0 items-center gap-2'>
                  <Input
                    data-testid='character-name'
                    value={selected.name}
                    maxLength={MAX_NAME}
                    placeholder={t('characters.namePlaceholder')}
                    onChange={(e) => update(selected.id, { name: e.target.value })}
                    className='h-8 text-sm'
                    autoFocus={!selected.name}
                  />
                  <Button
                    data-testid='character-delete'
                    aria-label={t('characters.delete')}
                    variant='ghost'
                    size='icon-sm'
                    onClick={() => remove(selected.id)}
                    className='text-rose-600 hover:text-rose-600'
                  >
                    <Trash2Icon className='size-4' />
                  </Button>
                </div>
                <div
                  className='grid shrink-0 grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-2 gap-y-1.5 text-xs'
                  data-testid='character-style'
                >
                  <span className='text-muted-foreground'>{t('characters.font')}</span>
                  <Input
                    data-testid='character-font'
                    list='character-font-options'
                    value={selected.font ?? ''}
                    placeholder={t('characters.fontPlaceholder')}
                    onChange={(e) => update(selected.id, { font: e.target.value || null })}
                    className='h-7 text-xs'
                  />
                  <datalist id='character-font-options'>
                    {fonts.map((f) => (
                      <option key={f.postScriptName} value={f.postScriptName}>
                        {f.familyName}
                      </option>
                    ))}
                  </datalist>
                  {selected.font ? (
                    <Button
                      aria-label={t('characters.clearFont')}
                      variant='ghost'
                      size='icon-xs'
                      onClick={() => update(selected.id, { font: null })}
                    >
                      <XIcon className='size-3' />
                    </Button>
                  ) : (
                    <span className='w-6' />
                  )}
                  <span className='text-muted-foreground'>{t('characters.color')}</span>
                  <div className='flex items-center gap-2'>
                    <ColorPicker
                      value={selected.color ? rgbaToHex(selected.color) : '#000000'}
                      onChange={(hex) => update(selected.id, { color: hexToRgba(hex) })}
                      triggerTestId='character-color'
                      className='size-7'
                    />
                    <span className='text-muted-foreground' data-testid='character-color-value'>
                      {selected.color ? rgbaToHex(selected.color) : t('characters.styleNotSet')}
                    </span>
                  </div>
                  {selected.color ? (
                    <Button
                      aria-label={t('characters.clearColor')}
                      variant='ghost'
                      size='icon-xs'
                      onClick={() => update(selected.id, { color: null })}
                    >
                      <XIcon className='size-3' />
                    </Button>
                  ) : (
                    <span className='w-6' />
                  )}
                  <span className='text-muted-foreground'>{t('characters.size')}</span>
                  <div className='flex items-center gap-2'>
                    <Input
                      data-testid='character-size'
                      type='number'
                      min={1}
                      max={1000}
                      step={0.5}
                      value={selected.fontSizePx ?? ''}
                      placeholder={t('characters.sizePlaceholder')}
                      onChange={(e) => {
                        const v = Number.parseFloat(e.target.value)
                        update(selected.id, {
                          fontSizePx: Number.isFinite(v) && v > 0 ? v : null,
                        })
                      }}
                      className='h-7 w-28 text-xs'
                    />
                    <span className='text-[10px] text-muted-foreground'>
                      {referencePageHeight
                        ? t('characters.sizeHintRef', { height: referencePageHeight })
                        : t('characters.sizeHint')}
                    </span>
                  </div>
                  {selected.fontSizePx ? (
                    <Button
                      aria-label={t('characters.clearSize')}
                      variant='ghost'
                      size='icon-xs'
                      onClick={() => update(selected.id, { fontSizePx: null })}
                    >
                      <XIcon className='size-3' />
                    </Button>
                  ) : (
                    <span className='w-6' />
                  )}
                  <span className='text-muted-foreground'>{t('characters.effect')}</span>
                  <div className='flex items-center gap-1'>
                    {(
                      [
                        { key: 'bold', Icon: BoldIcon, label: t('render.effectBold') },
                        { key: 'italic', Icon: ItalicIcon, label: t('render.effectItalic') },
                      ] as const
                    ).map(({ key, Icon, label }) => (
                      <Button
                        key={key}
                        aria-label={label}
                        aria-pressed={!!selected[key]}
                        data-testid={`character-${key}`}
                        variant={selected[key] ? 'default' : 'outline'}
                        size='icon-xs'
                        onClick={() => {
                          const next = selected[key] ? null : true
                          update(selected.id, key === 'bold' ? { bold: next } : { italic: next })
                        }}
                      >
                        <Icon className='size-3' />
                      </Button>
                    ))}
                    <span className='mx-1 h-4 w-px bg-border' />
                    {(
                      [
                        { value: 'left', Icon: AlignLeftIcon, label: t('render.alignLeft') },
                        { value: 'center', Icon: AlignCenterIcon, label: t('render.alignCenter') },
                        { value: 'right', Icon: AlignRightIcon, label: t('render.alignRight') },
                      ] as { value: Align; Icon: typeof AlignLeftIcon; label: string }[]
                    ).map(({ value, Icon, label }) => (
                      <Button
                        key={value}
                        aria-label={label}
                        aria-pressed={selected.textAlign === value}
                        data-testid={`character-align-${value}`}
                        variant={selected.textAlign === value ? 'default' : 'outline'}
                        size='icon-xs'
                        onClick={() =>
                          update(selected.id, {
                            textAlign: selected.textAlign === value ? null : value,
                          })
                        }
                      >
                        <Icon className='size-3' />
                      </Button>
                    ))}
                  </div>
                  <span className='w-6' />
                </div>
                <div className='flex shrink-0 items-center gap-2'>
                  <Button
                    variant='outline'
                    size='xs'
                    className='text-[11px]'
                    data-testid='character-copy-style'
                    disabled={
                      !selectedStyle ||
                      (!selectedStyle.font &&
                        !selectedStyle.color &&
                        !selectedStyle.fontSize &&
                        selectedStyle.bold == null &&
                        selectedStyle.italic == null &&
                        !selectedStyle.textAlign)
                    }
                    onClick={() => {
                      if (!selectedStyle) return
                      // Block sizes are page px; character sizes are measured
                      // on the project's reference page height.
                      const size =
                        selectedStyle.fontSize && referencePageHeight && selectedStyle.pageHeight
                          ? (selectedStyle.fontSize * referencePageHeight) / selectedStyle.pageHeight
                          : selectedStyle.fontSize
                      update(selected.id, {
                        font: selectedStyle.font ?? selected.font ?? null,
                        color: selectedStyle.color ?? selected.color ?? null,
                        fontSizePx: size ? Math.round(size * 10) / 10 : (selected.fontSizePx ?? null),
                        bold: selectedStyle.bold ?? selected.bold ?? null,
                        italic: selectedStyle.italic ?? selected.italic ?? null,
                        textAlign: selectedStyle.textAlign ?? selected.textAlign ?? null,
                      })
                    }}
                  >
                    <CopyIcon className='size-3' />
                    {t('characters.copyStyle')}
                  </Button>
                  <span className='text-[10px] text-muted-foreground'>
                    {t('characters.styleHint')}
                  </span>
                </div>
                {usage[selected.id] ? (
                  <p className='shrink-0 text-[10px] text-muted-foreground'>
                    {t('characters.deleteWarning', { count: usage[selected.id] })}
                  </p>
                ) : null}
                <Textarea
                  data-testid='character-description'
                  value={selected.description}
                  maxLength={MAX_DESCRIPTION}
                  placeholder={t('characters.descriptionPlaceholder')}
                  onChange={(e) => update(selected.id, { description: e.target.value })}
                  className='min-h-0 flex-1 resize-none px-3 py-2 text-xs leading-relaxed'
                />
                <p className='shrink-0 text-right text-[10px] text-muted-foreground tabular-nums'>
                  {selected.description.length} / {MAX_DESCRIPTION}
                </p>
              </>
            ) : (
              <div className='flex flex-1 items-center justify-center rounded-md border border-dashed border-border text-xs text-muted-foreground'>
                {t('characters.selectPrompt')}
              </div>
            )}
          </div>
        </div>

        <div className='flex shrink-0 items-center justify-end gap-2'>
          {error && <p className='mr-auto text-xs text-rose-600'>{error}</p>}
          <Button variant='ghost' size='sm' onClick={() => onOpenChange(false)}>
            {t('characters.cancel')}
          </Button>
          <Button
            size='sm'
            onClick={() => void handleSave()}
            disabled={!initialized || save.isPending}
            data-testid='character-save'
          >
            {save.isPending && <Loader2Icon className='size-3.5 animate-spin' />}
            {t('characters.save')}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
