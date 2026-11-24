// What changed in a Files list between two loads, and where each row is drawn, so a live refresh
// can mark the rows that moved (DECISIONS D5) instead of redrawing the list.
import type { ChangedFile } from './types';

/** What one load changed against the load before it. */
export type ListChanges = {
  /** Paths listed now that were not before. */
  added: ReadonlySet<string>;
  /** Paths whose status or counts moved. */
  changed: ReadonlySet<string>;
  /** Files listed before and no more (drawn in place a moment while they fade). */
  removed: readonly ChangedFile[];
};

export type Mark = 'added' | 'changed' | 'removed' | null;
export type FileRow = { file: ChangedFile; mark: Mark };

export const NO_CHANGES: ListChanges = { added: new Set(), changed: new Set(), removed: [] };

/** What `next` changed against `previous`; `previous` undefined (the first load) marks nothing. */
export function listChanges(previous: readonly ChangedFile[] | undefined, next: readonly ChangedFile[]): ListChanges {
  if (!previous) return NO_CHANGES;
  const before = new Map(previous.map((f) => [f.path, f]));
  const now = new Set(next.map((f) => f.path));
  const added = new Set<string>();
  const changed = new Set<string>();
  for (const f of next) {
    const old = before.get(f.path);
    if (!old) added.add(f.path);
    else if (countsMoved(old, f)) changed.add(f.path);
  }
  return { added, changed, removed: previous.filter((f) => !now.has(f.path)) };
}

function countsMoved(a: ChangedFile, b: ChangedFile): boolean {
  return a.status !== b.status || (a.additions ?? 0) !== (b.additions ?? 0) || (a.deletions ?? 0) !== (b.deletions ?? 0) || !!a.binary !== !!b.binary || !!a.untracked !== !!b.untracked;
}

/**
 * The rows to draw: `next` in its order, with each removed file put back where it stood in
 * `previous` (after the nearest earlier file that is still listed), so nothing below it jumps.
 */
export function fileRows(previous: readonly ChangedFile[] | undefined, next: readonly ChangedFile[], changes: ListChanges): FileRow[] {
  const mark = (f: ChangedFile): Mark => (changes.added.has(f.path) ? 'added' : changes.changed.has(f.path) ? 'changed' : null);
  if (!changes.removed.length || !previous) return next.map((file) => ({ file, mark: mark(file) }));
  const removed = new Set(changes.removed.map((f) => f.path));
  // The removed files that follow each file still listed ('' = before the first one).
  const after = new Map<string, ChangedFile[]>();
  let anchor = '';
  for (const f of previous) {
    if (!removed.has(f.path)) anchor = f.path;
    else after.set(anchor, [...(after.get(anchor) ?? []), f]);
  }
  const gone = (path: string) => (after.get(path) ?? []).map((file): FileRow => ({ file, mark: 'removed' }));
  const rows = gone('');
  for (const file of next) rows.push({ file, mark: mark(file) }, ...gone(file.path));
  return rows;
}
