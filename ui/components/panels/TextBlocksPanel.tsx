'use client'

import {
  BookOpenIcon,
  CheckCheckIcon,
  LibraryIcon,
  MessageSquarePenIcon,
  Languages,
  LoaderCircleIcon,
  Trash2Icon,
  UsersIcon,
  WandSparklesIcon,
} from 'lucide-react'
import { motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { CharacterManagerDialog } from '@/components/CharacterManagerDialog'
import { GlossaryDialog } from '@/components/GlossaryDialog'
import { LibraryDialog } from '@/components/LibraryDialog'
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from '@/components/ui/accordion'
import { Button } from '@/components/ui/button'
import { DraftTextarea } from '@/components/ui/draft-textarea'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useCurrentPage, useTextNodes, type TextNodeEntry } from '@/hooks/useCurrentPage'
import { getConfig, startPipeline, useGetCurrentLlm } from '@/lib/api/default/default'
import { fetchApi } from '@/lib/api/fetch'
import type { TextDataPatch } from '@/lib/api/schemas'
import {
  isSuggested,
  speakerOf,
  useAssignSpeaker,
  useCharacters,
  useConfirmSpeakers,
  useGuessSpeakers,
  type Character,
  type CharacterBook,
} from '@/lib/characters'
import { applyOp, invalidateScene, queueAutoRender, reorderPageTextNodes } from '@/lib/io/scene'
import { ops } from '@/lib/ops'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { useJobsStore } from '@/lib/stores/jobsStore'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'
import { useSelectionStore } from '@/lib/stores/selectionStore'

export function TextBlocksPanel() {
  const { t } = useTranslation()
  const page = useCurrentPage()
  const textNodes = useTextNodes()
  useEffect(() => {
    if (process.env.NODE_ENV !== 'production') {
      console.debug(
        '[reorder] Text nodes order:',
        textNodes.map((n) => n.id),
      )
    }
  }, [textNodes])
  const selectedIds = useSelectionStore((s) => s.nodeIds)
  const select = useSelectionStore((s) => s.select)
  const clearSelection = useSelectionStore((s) => s.clear)
  const { data: llm } = useGetCurrentLlm()
  const llmReady = llm?.status === 'ready'
  const isProcessing = useJobsStore((s) =>
    Object.values(s.jobs).some((j) => j.status === 'running'),
  )
  const readingOrder = useEditorUiStore((s) => s.readingOrder)
  const setReadingOrder = useEditorUiStore((s) => s.setReadingOrder)
  const characterBook = useCharacters()
  const assignSpeaker = useAssignSpeaker()
  const guess = useGuessSpeakers()
  const confirm = useConfirmSpeakers()
  const [charactersOpen, setCharactersOpen] = useState(false)
  const [glossaryOpen, setGlossaryOpen] = useState(false)
  const [libraryOpen, setLibraryOpen] = useState(false)

  if (!page) {
    return (
      <div className='flex flex-1 items-center justify-center text-xs text-muted-foreground'>
        {t('textBlocks.emptyPrompt')}
      </div>
    )
  }

  const selectedIndex = textNodes.findIndex((n) => selectedIds.has(n.id))
  const accordionValue = selectedIndex >= 0 ? selectedIndex.toString() : ''

  const patchText = async (nodeId: string, patch: TextDataPatch) => {
    await applyOp(
      ops.updateNode(page.id, nodeId, {
        data: { text: patch } as never,
      }),
    )
    queueAutoRender(page.id)
  }

  const removeNode = async (nodeId: string) => {
    const node = page.nodes[nodeId]
    if (!node) return
    const idx = Object.keys(page.nodes).indexOf(nodeId)
    await applyOp(ops.removeNode(page.id, nodeId, node, idx < 0 ? 0 : idx))
    clearSelection()
    queueAutoRender(page.id)
  }

  const setSpeaker = async (nodeId: string, characterId: string | null) => {
    try {
      // Applies to every selected block when the edited one is part of a
      // multi-selection (shift/ctrl-click), otherwise just this block.
      const nodeIds = selectedIds.has(nodeId) ? Array.from(selectedIds) : [nodeId]
      await assignSpeaker.mutateAsync({ nodeIds, characterId })
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  // Why "Guess speakers" can't run right now (shown under the toolbar).
  const guessBlockedReason =
    characterBook.characters.length === 0
      ? t('characters.guessNeedsCharacters')
      : !llmReady
        ? t('characters.guessNeedsLlm')
        : textNodes.length === 0
          ? t('characters.guessNeedsText')
          : null

  const suggestedOnPage = textNodes
    .map((n) => n.id)
    .filter((id) => isSuggested(characterBook, id))

  const guessPage = async () => {
    try {
      const result = await guess.mutateAsync({
        pageId: page.id,
        targetLanguage: useEditorUiStore.getState().selectedLanguage,
      })
      if (result.considered > 0 && result.assigned === 0) {
        useEditorUiStore.getState().showError(t('characters.guessNoResult'))
      }
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  const confirmPage = async () => {
    try {
      await confirm.mutateAsync(suggestedOnPage)
    } catch (err) {
      useEditorUiStore.getState().showError(String(err))
    }
  }

  const generate = async (nodeId: string, instruction?: string) => {
    if (!page) return
    const cfg = await getConfig()
    const translator = cfg.pipeline?.translator || 'llm'
    const renderer = cfg.pipeline?.renderer || 'koharu-renderer'
    const editor = useEditorUiStore.getState()
    const prefs = usePreferencesStore.getState()
    // Keep rendering page-scoped, but constrain translation to the clicked block.
    await startPipeline({
      steps: [translator, renderer],
      pages: [page.id],
      textNodeIds: [nodeId],
      instruction: instruction?.trim() || undefined,
      targetLanguage: editor.selectedLanguage,
      systemPrompt: prefs.customSystemPrompt,
      defaultFont: prefs.defaultFont,
      readingOrder: editor.readingOrder === 'custom' ? undefined : editor.readingOrder,
    })
  }

  return (
    <div className='flex min-h-0 flex-1 flex-col' data-testid='panels-textblocks'>
      <div className='flex items-center justify-between border-b border-border px-2 py-1.5 text-xs font-semibold tracking-wide text-muted-foreground uppercase'>
        <span data-testid='textblocks-count' data-count={textNodes.length}>
          {t('textBlocks.title', { count: textNodes.length })}
        </span>
        <div className='flex items-center gap-1.5'>
          <span className='font-normal uppercase opacity-50'>{t('textBlocks.readingOrder')}:</span>
          <Select
            value={readingOrder}
            onValueChange={async (val: 'rtl' | 'ltr' | 'custom') => {
              if (process.env.NODE_ENV !== 'production') {
                console.debug('[reorder] Changing reading order to:', val)
              }

              if (val === 'custom') {
                setReadingOrder(val)
                return
              }

              try {
                await reorderPageTextNodes(page.id, val)
                setReadingOrder(val)
              } catch (err) {
                console.error('[reorder] Failed to reorder text nodes:', err)
                useEditorUiStore.getState().showError(String(err))
              }
            }}
          >
            <SelectTrigger
              className='h-5 w-32 gap-1 border-none bg-transparent px-1.5 text-[10px] font-semibold uppercase hover:bg-accent focus:ring-0'
              aria-label={t('textBlocks.readingOrder')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value='rtl' className='text-[10px] font-semibold'>
                {t('textBlocks.readingOrderRtl')}
              </SelectItem>
              <SelectItem value='ltr' className='text-[10px] font-semibold'>
                {t('textBlocks.readingOrderLtr')}
              </SelectItem>
              <SelectItem value='custom' className='text-[10px] font-semibold'>
                {t('textBlocks.readingOrderCustom')}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>
      <div
        className='flex flex-wrap items-center gap-1 border-b border-border px-2 py-1'
        data-testid='textblocks-speaker-toolbar'
      >
        <Button
          data-testid='textblocks-characters'
          variant='ghost'
          size='xs'
          onClick={() => setCharactersOpen(true)}
          className='text-[11px]'
        >
          <UsersIcon className='size-3' />
          {t('characters.manageShort')}
        </Button>
        <Button
          data-testid='textblocks-glossary'
          variant='ghost'
          size='xs'
          onClick={() => setGlossaryOpen(true)}
          className='text-[11px]'
        >
          <BookOpenIcon className='size-3' />
          {t('glossary.button')}
        </Button>
        <Button
          data-testid='textblocks-library'
          variant='ghost'
          size='xs'
          onClick={() => setLibraryOpen(true)}
          className='text-[11px]'
        >
          <LibraryIcon className='size-3' />
          {t('library.button')}
        </Button>
        <Button
          data-testid='textblocks-guess-speakers'
          variant='ghost'
          size='xs'
          disabled={guessBlockedReason !== null || isProcessing || guess.isPending}
          onClick={() => void guessPage()}
          className='text-[11px]'
        >
          {guess.isPending ? (
            <LoaderCircleIcon className='size-3 animate-spin' />
          ) : (
            <WandSparklesIcon className='size-3' />
          )}
          {t('characters.guessShort')}
        </Button>
        {suggestedOnPage.length > 0 && (
          <Button
            data-testid='textblocks-confirm-speakers'
            variant='ghost'
            size='xs'
            disabled={confirm.isPending}
            onClick={() => void confirmPage()}
            className='text-[11px] text-sky-500 hover:text-sky-500'
          >
            <CheckCheckIcon className='size-3' />
            {t('characters.confirmShort', { count: suggestedOnPage.length })}
          </Button>
        )}
        {guessBlockedReason && (
          <span
            data-testid='textblocks-guess-hint'
            className='basis-full px-1 text-[10px] text-muted-foreground'
          >
            {guessBlockedReason}
          </span>
        )}
      </div>
      <ScrollArea
        key={page.id}
        className='min-h-0 flex-1'
        viewportClassName='pb-1'
        data-testid='textblocks-scroll'
      >
        <div className='p-2'>
          {textNodes.length === 0 ? (
            <p className='rounded-md border border-dashed border-border p-2 text-xs text-muted-foreground'>
              {t('textBlocks.none')}
            </p>
          ) : (
            <Accordion
              data-testid='textblocks-accordion'
              type='single'
              collapsible
              value={accordionValue}
              onValueChange={(value) => {
                if (!value) {
                  clearSelection()
                  return
                }
                const idx = Number(value)
                const node = textNodes[idx]
                if (node) select(node.id, false)
              }}
              className='flex flex-col gap-1'
            >
              {textNodes.map((node, index) => (
                <BlockCard
                  key={node.id}
                  node={node}
                  index={index}
                  selected={selectedIds.has(node.id)}
                  onToggleSelect={() => select(node.id, true)}
                  onPatch={(patch) => void patchText(node.id, patch)}
                  onDelete={() => void removeNode(node.id)}
                  onGenerate={() => void generate(node.id)}
                  onRevise={(instruction) => void generate(node.id, instruction)}
                  processing={isProcessing}
                  llmReady={llmReady}
                  characterBook={characterBook}
                  onSpeakerChange={(characterId) => void setSpeaker(node.id, characterId)}
                />
              ))}
            </Accordion>
          )}
        </div>
      </ScrollArea>
      <CharacterManagerDialog
        open={charactersOpen}
        onOpenChange={setCharactersOpen}
        selectedStyle={
          selectedIndex >= 0
            ? {
                font: textNodes[selectedIndex].data.style?.fontFamilies?.[0] ?? null,
                color: textNodes[selectedIndex].data.style?.color ?? null,
                fontSize: textNodes[selectedIndex].data.style?.fontSize ?? null,
                bold: textNodes[selectedIndex].data.style?.effect?.bold ?? null,
                italic: textNodes[selectedIndex].data.style?.effect?.italic ?? null,
                textAlign: textNodes[selectedIndex].data.style?.textAlign ?? null,
                pageHeight: page?.height ?? null,
              }
            : undefined
        }
      />
      <GlossaryDialog open={glossaryOpen} onOpenChange={setGlossaryOpen} />
      <LibraryDialog open={libraryOpen} onOpenChange={setLibraryOpen} />
    </div>
  )
}

const UNKNOWN_SPEAKER = '__unknown__'

type BlockCardProps = {
  node: TextNodeEntry
  index: number
  selected: boolean
  onToggleSelect: () => void
  onPatch: (patch: TextDataPatch) => void
  onDelete: () => void
  onGenerate: () => void
  onRevise: (instruction: string) => void
  processing: boolean
  llmReady: boolean
  characterBook: CharacterBook
  onSpeakerChange: (characterId: string | null) => void
}

function BlockCard({
  node,
  index,
  selected,
  onToggleSelect,
  onPatch,
  onDelete,
  onGenerate,
  onRevise,
  processing,
  llmReady,
  characterBook,
  onSpeakerChange,
}: BlockCardProps) {
  const { t } = useTranslation()
  const data = node.data
  const hasOcr = !!data.text?.trim()
  const hasTranslation = !!data.translation?.trim()
  const preview = data.translation?.trim() || data.text?.trim()
  const speaker: Character | null = speakerOf(characterBook, node.id)
  const guessed = speaker !== null && isSuggested(characterBook, node.id)

  return (
    <motion.div
      data-testid={`textblock-card-${index}`}
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2, delay: index * 0.03 }}
    >
      <AccordionItem
        value={index.toString()}
        data-selected={selected}
        className='overflow-hidden rounded-md bg-card/90 text-xs ring-1 ring-border data-[selected=true]:ring-primary'
      >
        <AccordionTrigger
          onClick={(e) => {
            if (e.shiftKey || e.ctrlKey || e.metaKey) {
              e.preventDefault()
              e.stopPropagation()
              onToggleSelect()
            }
          }}
          className='flex w-full cursor-pointer items-center gap-1.5 px-2 py-1.5 text-left transition outline-none hover:no-underline data-[state=open]:bg-accent [&>svg]:hidden'
        >
          <span
            className={`shrink-0 rounded-md px-1.5 py-0.5 text-center text-[10px] font-medium text-white tabular-nums ${
              selected ? 'bg-primary' : 'bg-muted-foreground/60'
            }`}
            style={{ minWidth: '1.5rem' }}
          >
            {index + 1}
          </span>
          <div className='flex min-w-0 flex-1 items-center gap-1'>
            <span
              className={`shrink-0 rounded-sm px-1 py-0.5 text-[9px] font-medium uppercase ${
                hasOcr ? 'bg-rose-400/70 text-white' : 'bg-muted text-muted-foreground/50'
              }`}
            >
              {t('textBlocks.ocrBadge')}
            </span>
            <span
              className={`shrink-0 rounded-sm px-1 py-0.5 text-[9px] font-medium uppercase ${
                hasTranslation ? 'bg-rose-400/70 text-white' : 'bg-muted text-muted-foreground/50'
              }`}
            >
              {t('textBlocks.translationBadge')}
            </span>
            {speaker && (
              <span
                data-testid={`textblock-speaker-chip-${index}`}
                data-suggested={guessed}
                className={`max-w-20 shrink-0 truncate rounded-sm px-1 py-0.5 text-[9px] font-medium ${
                  guessed
                    ? 'border border-dashed border-sky-500 text-sky-600 dark:text-sky-300'
                    : 'bg-sky-500/70 text-white'
                }`}
                title={guessed ? `${speaker.name} (${t('characters.suggestedHint')})` : speaker.name}
              >
                {speaker.name}
                {guessed && '?'}
              </span>
            )}
            {preview && (
              <p className='line-clamp-1 min-w-0 flex-1 text-xs text-muted-foreground'>{preview}</p>
            )}
          </div>
        </AccordionTrigger>
        <AccordionContent className='px-2 pt-1.5 pb-2 shadow-[inset_0_1px_0_0_var(--color-border)]'>
          <div className='space-y-1.5'>
            <div className='flex flex-col gap-0.5'>
              <span className='text-[10px] text-muted-foreground uppercase'>
                {t('textBlocks.ocrLabel')}
              </span>
              <DraftTextarea
                data-testid={`textblock-ocr-${index}`}
                value={data.text ?? ''}
                placeholder={t('textBlocks.addOcrPlaceholder')}
                rows={2}
                onValueChange={(value) => onPatch({ text: value })}
                className='min-h-0 resize-none px-1.5 py-1 text-xs'
              />
            </div>
            <div className='flex flex-col gap-0.5'>
              <div className='flex items-center justify-between'>
                <span className='text-[10px] text-muted-foreground uppercase'>
                  {t('textBlocks.translationLabel')}
                </span>
                <div className='flex items-center gap-0.5'>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        data-testid={`textblock-delete-${index}`}
                        aria-label={t('workspace.deleteBlock')}
                        variant='ghost'
                        size='icon-xs'
                        disabled={processing}
                        onClick={onDelete}
                        className='size-5 text-rose-600 hover:text-rose-600'
                      >
                        <Trash2Icon className='size-3' />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side='left' sideOffset={4}>
                      {t('workspace.deleteBlock')}
                    </TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        data-testid={`textblock-generate-${index}`}
                        aria-label={t('llm.generateTooltip')}
                        variant='ghost'
                        size='icon-xs'
                        disabled={!llmReady || processing}
                        onClick={onGenerate}
                        className='size-5'
                      >
                        {processing ? (
                          <LoaderCircleIcon className='size-3 animate-spin' />
                        ) : (
                          <Languages className='size-3' />
                        )}
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side='left' sideOffset={4}>
                      {t('llm.generateTooltip')}
                    </TooltipContent>
                  </Tooltip>
                  <ReviseButton
                    index={index}
                    disabled={!llmReady || processing || !hasOcr}
                    onRevise={onRevise}
                  />
                </div>
              </div>
              <DraftTextarea
                data-testid={`textblock-translation-${index}`}
                value={data.translation ?? ''}
                placeholder={t('textBlocks.addTranslationPlaceholder')}
                rows={2}
                onValueChange={(value) => onPatch({ translation: value })}
                className='min-h-0 resize-none px-1.5 py-1 text-xs'
              />
            </div>
            <div className='flex flex-col gap-0.5'>
              <span className='text-[10px] text-muted-foreground uppercase'>
                {t('characters.speakerLabel')}
              </span>
              <Select
                value={speaker?.id ?? UNKNOWN_SPEAKER}
                onValueChange={(value) =>
                  onSpeakerChange(value === UNKNOWN_SPEAKER ? null : value)
                }
              >
                <SelectTrigger
                  data-testid={`textblock-speaker-${index}`}
                  aria-label={t('characters.speakerLabel')}
                  className='h-7 w-full px-1.5 text-xs'
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={UNKNOWN_SPEAKER} className='text-xs text-muted-foreground'>
                    {t('characters.unknownSpeaker')}
                  </SelectItem>
                  {characterBook.characters.map((c) => (
                    <SelectItem key={c.id} value={c.id} className='text-xs'>
                      {c.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        </AccordionContent>
      </AccordionItem>
    </motion.div>
  )
}

const REVISE_HISTORY_KEY = 'koharu.reviseHistory'
const REVISE_HISTORY_MAX = 6

const loadReviseHistory = (): string[] => {
  try {
    const raw = localStorage.getItem(REVISE_HISTORY_KEY)
    const list = raw ? (JSON.parse(raw) as unknown) : []
    return Array.isArray(list) ? list.filter((x): x is string => typeof x === 'string') : []
  } catch {
    return []
  }
}

const saveReviseHistory = (instruction: string) => {
  try {
    const next = [instruction, ...loadReviseHistory().filter((x) => x !== instruction)].slice(
      0,
      REVISE_HISTORY_MAX,
    )
    localStorage.setItem(REVISE_HISTORY_KEY, JSON.stringify(next))
  } catch {
    // Storage unavailable: the history is only a convenience.
  }
}

/** Re-translate one block with a short request ("more polite", ...). */
function ReviseButton({
  index,
  disabled,
  onRevise,
}: {
  index: number
  disabled: boolean
  onRevise: (instruction: string) => void
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [instruction, setInstruction] = useState('')
  const [history, setHistory] = useState<string[]>([])

  useEffect(() => {
    if (open) setHistory(loadReviseHistory())
  }, [open])

  const submit = (value: string) => {
    const text = value.trim()
    if (!text) return
    saveReviseHistory(text)
    onRevise(text)
    setOpen(false)
    setInstruction('')
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <Button
              data-testid={`textblock-revise-${index}`}
              aria-label={t('textBlocks.revise')}
              variant='ghost'
              size='icon-xs'
              disabled={disabled}
              className='size-5'
            >
              <MessageSquarePenIcon className='size-3' />
            </Button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent side='left' sideOffset={4}>
          {t('textBlocks.revise')}
        </TooltipContent>
      </Tooltip>
      <PopoverContent side='left' align='start' className='w-72 p-2'>
        <div className='flex flex-col gap-1.5'>
          <span className='text-xs font-medium'>{t('textBlocks.reviseTitle')}</span>
          <textarea
            data-testid={`textblock-revise-input-${index}`}
            autoFocus
            value={instruction}
            maxLength={1000}
            rows={2}
            placeholder={t('textBlocks.revisePlaceholder')}
            onChange={(e) => setInstruction(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault()
                submit(instruction)
              }
            }}
            className='w-full resize-none rounded-md border border-input bg-transparent px-2 py-1 text-xs outline-none focus-visible:ring-1 focus-visible:ring-ring'
          />
          {history.length > 0 && (
            <div className='flex flex-wrap gap-1'>
              {history.map((h) => (
                <button
                  key={h}
                  type='button'
                  onClick={() => submit(h)}
                  className='max-w-full truncate rounded-full border border-border px-2 py-0.5 text-[10px] text-muted-foreground hover:bg-accent hover:text-foreground'
                  title={h}
                >
                  {h}
                </button>
              ))}
            </div>
          )}
          <div className='flex items-center justify-between gap-2'>
            <span className='text-[10px] text-muted-foreground'>{t('textBlocks.reviseHint')}</span>
            <Button
              size='xs'
              data-testid={`textblock-revise-submit-${index}`}
              disabled={!instruction.trim()}
              onClick={() => submit(instruction)}
            >
              {t('textBlocks.reviseSubmit')}
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
