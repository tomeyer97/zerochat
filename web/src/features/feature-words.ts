/**
 * A feature in words: its status in the list and the header, what needs a person on its page,
 * and the confirmations its actions show.
 */
import type { FeatureDetail, FeatureRow, SliceView } from '../types';
import type { Tone } from '../status';
import { REVIEW_WAIT, afterMerge, failureWord, list, waitVerb } from './slice-words';

/** A feature with a daemon or running slices changes by the second: poll it often. */
export function isLiveRow(row: Pick<FeatureRow, 'daemonPid' | 'running'>): boolean {
  return row.daemonPid != null || row.running.length > 0;
}

export type NowPart = { text: string; tone: Tone; slice?: string };

type NowRow = Pick<FeatureRow, 'paused' | 'approve' | 'failed' | 'running' | 'note' | 'merged' | 'skipped' | 'total' | 'active' | 'daemonPid' | 'started' | 'issue'>;

/** Every slice is merged or skipped. */
function allSettled(row: Pick<FeatureRow, 'merged' | 'skipped' | 'total'>): boolean {
  return row.total > 0 && row.merged + (row.skipped ?? 0) === row.total;
}

/**
 * `Paused after 2 failures in a row`, and the slice that failed last (zsf's long reason stays in
 * that slice's panel). Parses the pause reason zsf's engine writes (`engine.rs`: `{n}
 * infrastructure failures in a row (last: {id}: {why})`); anything else is shown as it is.
 */
export function pausedSummary(paused: string): { text: string; slice: string | null } {
  const m = /^(\d+) infrastructure failures? in a row(?: \(last: ([^:]+):)?/.exec(paused);
  if (!m) return { text: `Paused: ${paused}`, slice: null };
  return { text: `Paused after ${m[1]} failure${m[1] === '1' ? '' : 's'} in a row`, slice: m[2] ?? null };
}

/**
 * The feature's status in a few words for the list, most pressing first: what needs you, then
 * what runs, else one quiet word. `featureState` below is the header's one-word chip; the two
 * read the same row but differ on purpose: the list names every item that needs you and shows
 * running slices even when all are settled; the chip says `Done` before `Running`, and
 * `Stopped` for an inactive feature even while its daemon still runs.
 */
export function nowParts(row: NowRow): NowPart[] {
  const parts: NowPart[] = [];
  if (row.issue) parts.push({ text: "can't read the plan or state", tone: 'bad' });
  if (row.paused) {
    const p = pausedSummary(row.paused);
    parts.push({ text: p.text.toLowerCase(), tone: 'warn', slice: p.slice ?? undefined });
  }
  for (const id of row.approve) parts.push({ text: `${id} ${REVIEW_WAIT}`, tone: 'warn', slice: id });
  for (const id of row.failed) parts.push({ text: `${id} failed`, tone: 'bad', slice: id });
  if (row.note) parts.push({ text: row.note, tone: 'warn' });
  const done = allSettled(row);
  if (!done && row.active && row.daemonPid == null && !row.paused && !row.issue) parts.push({ text: 'not running', tone: 'warn' });
  if (row.running.length) parts.push({ text: `running ${list(row.running)}`, tone: 'running' });
  if (parts.length) return parts;
  if (done) return [{ text: row.skipped ? `done, ${row.skipped} skipped` : 'done', tone: 'muted' }];
  if (!row.started) return [{ text: 'not started', tone: 'muted' }];
  if (!row.active && row.daemonPid == null) return [{ text: 'stopped', tone: 'muted' }];
  return [{ text: 'idle', tone: 'muted' }];
}

/** One word for the header chip (see `nowParts` for how it differs from the list's status). */
export function featureState(row: NowRow & Pick<FeatureRow, 'needsYou'>): { label: string; tone: Tone } {
  if (row.issue) return { label: 'Unreadable', tone: 'bad' };
  if (row.paused) return { label: 'Paused', tone: 'warn' };
  if (row.needsYou) return { label: 'Needs you', tone: 'warn' };
  if (allSettled(row)) return { label: 'Done', tone: 'muted' };
  if (row.running.length) return { label: 'Running', tone: 'running' };
  if (!row.started) return { label: 'Not started', tone: 'muted' };
  if (!row.active) return { label: 'Stopped', tone: 'muted' };
  return { label: 'Idle', tone: 'muted' };
}

/** Needs you first, then running, then the rest; finished ones newest first. */
export function sortFeatures<T extends Pick<FeatureRow, 'name' | 'needsYou' | 'running' | 'finishedAt' | 'updatedAt'>>(rows: T[]): T[] {
  const rank = (r: T) => (r.needsYou ? 0 : r.running.length ? 1 : r.finishedAt == null ? 2 : 3);
  return [...rows].sort((a, b) => rank(a) - rank(b) || (b.updatedAt ?? 0) - (a.updatedAt ?? 0) || a.name.localeCompare(b.name));
}

export function progressText(row: Pick<FeatureRow, 'merged' | 'skipped' | 'total'>): string {
  return `${row.merged} of ${row.total} merged${row.skipped ? `, ${row.skipped} skipped` : ''}`;
}

type StopRow = Pick<FeatureDetail, 'finishedAt' | 'issue' | 'paused' | 'daemonPid' | 'started' | 'active'> & {
  slices: Pick<SliceView, 'id' | 'status' | 'attempts'>[];
};

/**
 * Why nothing runs, when the feature is unfinished, readable, not paused and has no daemon
 * (`null` otherwise). Amber when work waits (a lost daemon, or runs that ended and wait to merge).
 */
export function stoppedReason(detail: StopRow): { text: string; tone: 'warn' | 'muted' } | null {
  if (detail.finishedAt != null || detail.issue || detail.paused || detail.daemonPid != null) return null;
  // Runs that ended while nobody watched: zsf merges them on its next start.
  const runFinished = detail.slices.filter((s) => s.status === 'running' && s.attempts[s.attempts.length - 1]?.runEnded).map((s) => s.id);
  const tone = detail.active || runFinished.length ? 'warn' : 'muted';
  if (!detail.started) return { text: 'Not started yet.', tone };
  if (detail.active) return { text: 'Not running: its daemon is gone, so nothing starts or merges.', tone };
  if (runFinished.length) {
    const merges = runFinished.length > 1 ? 'merge' : 'merges';
    return { text: `Stopped: ${runFinished.join(', ')} finished and ${merges} when you start again.`, tone };
  }
  return { text: 'Stopped: nothing new starts until you start again.', tone };
}

type AttentionSlice = Pick<SliceView, 'id' | 'title' | 'deps' | 'waitsFor' | 'commit' | 'mergedAt' | 'status' | 'attempts'>;
type AttentionDetail = Pick<FeatureDetail, 'paused' | 'pausedAt' | 'approve' | 'failed' | 'note'> & Omit<StopRow, 'slices'> & { slices: AttentionSlice[] };

/** One thing that needs a person, in the order the feature page lists them. */
export type AttentionItem =
  /** zsf paused the feature; `slice` failed last. */
  | { kind: 'paused'; text: string; slice: string | null; reason: string; since: number | null }
  /**
   * A checkpoint waits for a person. `review`: its run is done, not merged (`is ready for your review`, then
   * `; 06 starts after it is merged`); else an older zsf merged it first (`is merged (abc1234) and waits for
   * your approval`, then `; 06 starts after it`).
   */
  | { kind: 'approve'; slice: string; title: string; review: boolean; waits: string; since: number | null; after: string }
  /** A failed slice: `gate failed`, then `; 06 waits for it`. */
  | { kind: 'failed'; slice: string; title: string; failure: string; after: string }
  /** zsf's own note on what holds the feature back (a lease, low disk). */
  | { kind: 'note'; text: string }
  /** Nothing runs: see `stoppedReason`. */
  | { kind: 'stopped'; text: string; tone: 'warn' | 'muted' };

/** What needs a person on a feature's page (an unreadable feature's issue is shown on its own). */
export function attentionItems(detail: AttentionDetail): AttentionItem[] {
  const byId = new Map(detail.slices.map((s) => [s.id, s]));
  const items: AttentionItem[] = [];
  if (detail.paused) {
    const p = pausedSummary(detail.paused);
    items.push({ kind: 'paused', text: p.text, slice: p.slice, reason: detail.paused, since: detail.pausedAt ?? null });
  }
  for (const id of detail.approve) {
    const s = byId.get(id);
    const dependents = detail.slices.filter((d) => d.deps.includes(id)).map((d) => d.id);
    const starts = dependents.length > 1 ? 'start' : 'starts';
    if (s?.status === 'waiting_for_review') {
      items.push({ kind: 'approve', slice: id, title: s.title, review: true, waits: 'is ready for your review', since: null, after: dependents.length ? `; ${afterMerge(dependents)}` : '' });
      continue;
    }
    items.push({
      kind: 'approve',
      slice: id,
      title: s?.title ?? '',
      review: false,
      waits: `is merged${s?.commit ? ` (${s.commit.slice(0, 7)})` : ''} and waits for your approval`,
      since: s?.mergedAt || null,
      after: dependents.length ? `; ${dependents.join(', ')} ${starts} after it` : '',
    });
  }
  for (const id of detail.failed) {
    const s = byId.get(id);
    const blocked = detail.slices.filter((d) => d.waitsFor.includes(id)).map((d) => d.id);
    items.push({ kind: 'failed', slice: id, title: s?.title ?? '', failure: s ? failureWord(s) : 'failed', after: blocked.length ? `; ${waitVerb(blocked)} for it` : '' });
  }
  if (detail.note) items.push({ kind: 'note', text: `Held back: ${detail.note}.` });
  const stopped = stoppedReason(detail);
  if (stopped) items.push({ kind: 'stopped', ...stopped });
  return items;
}

/** `03 approved` → `03 approved.`; first letter up. */
export function sentence(text: string): string {
  const t = text.trim();
  if (!t) return t;
  const up = /^\d/.test(t) ? t : t[0].toUpperCase() + t.slice(1);
  return /[.!?]$/.test(up) ? up : `${up}.`;
}

/**
 * An action's confirmation as a toast: a full stop at the end, and the first letter up unless
 * the message starts with the feature's name (`calc-v2 started.` keeps its case).
 */
export function toastText(message: string, feature: string): string {
  return message.startsWith(feature) ? message.trim().replace(/[.!?]?$/, '.') : sentence(message);
}

/** `#/…/slice/4` means 04: slice ids are zero-padded numbers people type without the zeros. */
export function findPaddedSlice<T extends Pick<SliceView, 'id'>>(slices: T[], id: string): T | undefined {
  if (!/^\d+$/.test(id)) return undefined;
  return slices.find((s) => /^\d+$/.test(s.id) && Number(s.id) === Number(id));
}
