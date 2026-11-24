// The Batches list: a row's text, and the two rows picked to compare (plus the plain
// tick/untick every Benchmarks picker uses).
import type { BatchSummary } from '../types';
import { costText, timeOfDay } from '../format';
import { retriedText } from './batches';
import { aggregateText } from './scores';

/** `harness v3 · claude-sonnet-5, claude-haiku-4-5`: the harness version (named, so its hash is
 * not read as a commit or a task set) and the models the batch ran. */
export function batchIdentity(b: Pick<BatchSummary, 'harnessVersion' | 'models'>): string {
  return [b.harnessVersion ? `harness ${b.harnessVersion}` : null, b.models.length > 0 ? b.models.join(', ') : null].filter(Boolean).join(' · ');
}

/** The batch's mean score with its range, over the cells done so far. */
export function batchScoreText(b: Pick<BatchSummary, 'id' | 'meanScore' | 'minScore' | 'maxScore' | 'runsDone'>): string {
  return aggregateText({ key: b.id, mean: b.meanScore, min: b.minScore, max: b.maxScore, n: b.runsDone });
}

/** Why the Score column shows `—` (`null` when it shows a score): a stopped or crashed batch
 * ended before any of its runs was scored. */
export function batchNoScoreReason(b: Pick<BatchSummary, 'id' | 'meanScore' | 'minScore' | 'maxScore' | 'runsDone' | 'status'>): string | null {
  if (batchScoreText(b) !== '—') return null;
  switch (b.status) {
    case 'stopped':
      return 'stopped before a run was scored';
    case 'crashed':
      return 'crashed before a run was scored';
    case 'failed_to_start':
      return 'never started';
    case 'running':
    case 'starting':
      return 'nothing scored yet';
    default:
      return 'no run was scored';
  }
}

/** Why the Cost column shows `—` (`null` when it shows a cost). */
export function batchNoCostReason(b: Pick<BatchSummary, 'totalCostUsd'>): string | null {
  return b.totalCostUsd == null ? 'pb recorded no cost for its runs' : null;
}

/** The Done column's tooltip: cells done of planned, and the retries that are not cells. */
export function batchDoneTitle(b: Pick<BatchSummary, 'runsDone' | 'runsTotal' | 'retriedAttempts' | 'retriedCostUsd'>): string {
  const retried = retriedText(b);
  return `${b.runsDone} of ${b.runsTotal} cells (task × seed) done${retried ? `; ${retried}, not counted as cells` : ''}`;
}

/** The row's numbers on one line, each with its label, for phone width, where the numeric
 * columns (and their headers) are hidden: `2 tasks × 3 seeds · 6/6 done +1 retried · score 0.71
 * (0.58–0.84) · $4.10 · started 12:25`; a score or cost that is missing says why. */
export function batchCompactText(b: BatchSummary, now: number): string {
  const done = `${b.runsDone}/${b.runsTotal} done${b.retriedAttempts > 0 ? ` +${b.retriedAttempts} retried` : ''}`;
  const noScore = batchNoScoreReason(b);
  const score = noScore ? `no score (${noScore})` : `score ${batchScoreText(b)}`;
  const cost = batchNoCostReason(b) ? 'no cost recorded' : costText(b.totalCostUsd, false);
  return `${plural(b.taskCount, 'task')} × ${plural(b.seedCount, 'seed')} · ${done} · ${score} · ${cost} · started ${timeOfDay(b.startedAt, now)}`;
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

/** Tick or untick `id` in a list of picked ids. */
export function toggleIn(picked: string[], id: string): string[] {
  return picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
}

/** Tick or untick a batch to compare. At most two stay picked: a third tick drops the first. */
export function toggleCompare(picked: string[], id: string): string[] {
  if (picked.includes(id) || picked.length < 2) return toggleIn(picked, id);
  return [picked[1], id];
}

/** The two picked batches as `[A, B]`, the older started first, so Δ = new − old whichever row
 * was ticked first; `null` until two are picked. */
export function comparePair(picked: string[], batches: Pick<BatchSummary, 'id' | 'startedAt'>[]): [string, string] | null {
  if (picked.length !== 2) return null;
  const startedAt = (id: string) => batches.find((b) => b.id === id)?.startedAt ?? 0;
  const [a, b] = [...picked].sort((x, y) => startedAt(x) - startedAt(y));
  return [a, b];
}
