'use client'

import { getGetSceneJsonQueryKey } from '@/lib/api/default/default'
import type { SceneSnapshot } from '@/lib/api/schemas'
import { openImageFiles, openImageFolder, openKhrFile } from '@/lib/io/openFiles'
import { isTauri } from '@/lib/backend'
import { useExportProgressStore } from '@/lib/io/exportProgress'
import { saveBlob } from '@/lib/io/saveBlob'
import { exportProject, uploadKhrArchive, uploadPages, uploadPagesByPaths } from '@/lib/io/scene'
import { queryClient } from '@/lib/queryClient'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

/**
 * Platform-neutral image import. `openImageFiles` / `openImageFolder` return
 * `File[]` on both Tauri and the web; the upload + scene invalidation lives
 * in `lib/io/scene.ts` on top of the orval-generated `createPages` mutation.
 */
export async function importPages(
  mode: 'replace' | 'append',
  source: 'files' | 'folder',
): Promise<void> {
  const picked = source === 'folder' ? await openImageFolder() : await openImageFiles()
  const replace = mode === 'replace'
  if (picked.kind === 'paths') {
    if (picked.paths.length === 0) return
    await uploadPagesByPaths(picked.paths, replace)
    return
  }
  if (picked.files.length === 0) return
  await uploadPages(picked.files, replace)
}

/**
 * Import a `.khr` archive. Works on both desktop and web: the archive file
 * is picked via the cross-platform `openKhrFile`, and the destination is
 * allocated server-side so the client never needs to touch the filesystem.
 */
export async function importKhrFile(): Promise<void> {
  const file = await openKhrFile()
  if (!file) return
  await uploadKhrArchive(file)
}

// ---------------------------------------------------------------------------
// Export (server returns bytes; saveBlob dispatches Tauri-dialog / web-FS)
// ---------------------------------------------------------------------------

const exportExtension: Record<'khr' | 'psd' | 'rendered' | 'inpainted', string> = {
  khr: 'khr',
  psd: 'zip',
  rendered: 'zip',
  inpainted: 'zip',
}

/** Sanitise an arbitrary project name for use as a filename stem. */
function sanitiseBaseName(name: string | undefined | null): string {
  const cleaned = (name ?? '')
    .trim()
    .replace(/[\\/:*?"<>|]+/g, '_')
    .replace(/\s+/g, ' ')
  return cleaned.length > 0 ? cleaned : 'koharu-export'
}

/** Read the current project name from React Query's cached scene snapshot. */
function currentProjectName(): string | undefined {
  const snap = queryClient.getQueryData<SceneSnapshot>(getGetSceneJsonQueryKey())
  return snap?.scene.project?.name ?? undefined
}

export async function exportCurrentProjectAs(
  format: 'khr' | 'psd' | 'rendered' | 'inpainted',
  pages?: string[],
): Promise<void> {
  // Several pages on the desktop: one request per page, written straight
  // into a folder chosen first. Building every page into one zip in memory
  // took minutes (or never finished) for long projects.
  if (isTauri() && format !== 'khr' && (pages === undefined || pages.length > 1)) {
    await exportPagesToFolder(format, pages)
    return
  }
  try {
    const defaultFont = usePreferencesStore.getState().defaultFont
    const { blob, filename } = await exportProject({ format, pages, defaultFont })
    const base = sanitiseBaseName(currentProjectName())
    // Prefer the server's Content-Disposition filename (matches the actual
    // bytes — a raw PNG/PSD for single-file responses, a zip for multi).
    // Fall back to our guess only if the header is missing/unparseable.
    const defaultName = filename ?? `${base}.${exportExtension[format]}`
    await saveBlob(blob, defaultName)
  } catch (err) {
    console.error('Export failed:', err)
    throw err
  }
}

const pageFileExtension: Record<'psd' | 'rendered' | 'inpainted', string> = {
  psd: 'psd',
  rendered: 'png',
  inpainted: 'png',
}

/**
 * Export pages one at a time into a folder picked up front, showing
 * progress (with Cancel). Files are named `001-<page name>.<ext>` in page
 * order; pages without the requested layer are skipped.
 */
async function exportPagesToFolder(
  format: 'psd' | 'rendered' | 'inpainted',
  pages?: string[],
): Promise<void> {
  const snap = queryClient.getQueryData<SceneSnapshot>(getGetSceneJsonQueryKey())
  const allPages = snap?.scene.pages ?? {}
  const ids = pages ?? Object.keys(allPages)
  if (ids.length === 0) return

  const { open } = await import('@tauri-apps/plugin-dialog')
  const { writeFile } = await import('@tauri-apps/plugin-fs')
  const folder = await open({ directory: true, multiple: false })
  if (!folder || typeof folder !== 'string') return

  const progress = useExportProgressStore.getState()
  progress.start(ids.length)
  const defaultFont = usePreferencesStore.getState().defaultFont
  const width = Math.max(3, String(ids.length).length)
  try {
    for (const [i, id] of ids.entries()) {
      if (useExportProgressStore.getState().cancelled) break
      const stem = sanitiseBaseName(allPages[id]?.name?.replace(/\.[^.]+$/, ''))
      const name = `${String(i + 1).padStart(width, '0')}-${stem}.${pageFileExtension[format]}`
      try {
        const { blob } = await exportProject({ format, pages: [id], defaultFont })
        await writeFile(`${folder}/${name}`, new Uint8Array(await blob.arrayBuffer()))
      } catch (err) {
        // A page without the layer (e.g. not rendered yet) is skipped.
        if (format === 'psd') throw err
        console.warn(`Export of page ${i + 1} skipped:`, err)
      }
      useExportProgressStore.getState().step()
    }
  } catch (err) {
    console.error('Export failed:', err)
    throw err
  } finally {
    useExportProgressStore.getState().finish()
  }
}
