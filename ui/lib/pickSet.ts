/**
 * Ticking many items in a list: one at a time, a Shift+click range, or all
 * the items shown at once. Pure helpers over a `Set` of item ids.
 */

/** Tick or untick every id in `ids`, leaving other ticked ids as they are. */
export function setMany(picked: Set<string>, ids: string[], on: boolean): Set<string> {
  const next = new Set(picked)
  for (const id of ids) {
    if (on) next.add(id)
    else next.delete(id)
  }
  return next
}

/** `'all'` when every id is ticked, `'some'` when only part, else `'none'`. */
export function pickState(picked: Set<string>, ids: string[]): 'all' | 'some' | 'none' {
  const count = ids.filter((id) => picked.has(id)).length
  if (ids.length > 0 && count === ids.length) return 'all'
  return count > 0 ? 'some' : 'none'
}

/**
 * Click on `id` in the list `ids` (in display order). Without Shift it flips
 * that item. With Shift and an `anchor` (the last item clicked) still in the
 * list, every item from the anchor to `id` takes the clicked item's new
 * state, like file lists do.
 */
export function pickClick(
  picked: Set<string>,
  ids: string[],
  id: string,
  shift: boolean,
  anchor: string | null,
): Set<string> {
  const on = !picked.has(id)
  const from = anchor === null ? -1 : ids.indexOf(anchor)
  const to = ids.indexOf(id)
  if (!shift || from < 0 || to < 0) return setMany(picked, [id], on)
  const [lo, hi] = from <= to ? [from, to] : [to, from]
  return setMany(picked, ids.slice(lo, hi + 1), on)
}
