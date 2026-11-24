// A batch as a whole, on its own page: whether it is live, what it has spent and could spend,
// its limits, its cell counts, and its state before pb has recorded it.
import type { BatchDetail, BatchSummary, BenchmarkCell } from '../types';
import { fullTime, timeOfDay, usd } from '../format';
import { cutShortBadge } from './batch-cells';
import { countByGroup, type CellGroup } from './cell-states';
import { startedRecently } from './started-batches';

/** A batch (or its matrix) still has something in flight: the list and the matrix poll while
 * this is true, and stop once every run has finished. */
export function batchIsLive(batch: Pick<BatchSummary, 'status' | 'runsInFlight'>): boolean {
  return batch.status === 'running' || batch.runsInFlight > 0;
}

export function detailIsLive(detail: Pick<BatchDetail, 'status'> & { cells: Pick<BenchmarkCell, 'state'>[] }): boolean {
  return detail.status === 'running' || detail.status === 'starting' || detail.cells.some((c) => c.state === 'running_agent' || c.state === 'evaluating');
}

/** `+3 retried attempts, $3.50`: attempts beyond one per cell (a retry of an interrupted or
 * crashed run), whose spend is in the batch total but whose rows are not cells. */
export function retriedText(b: { retriedAttempts: number; retriedCostUsd: number | null }): string | null {
  if (!b.retriedAttempts) return null;
  const n = `+${b.retriedAttempts} retried attempt${b.retriedAttempts === 1 ? '' : 's'}`;
  return b.retriedCostUsd != null ? `${n}, ${usd(b.retriedCostUsd)}` : n;
}

/** The most `runs` runs can spend: each stops at the same per-run cap. Both maximum-spend
 * figures (the Start form's `maxSpend` and the batch page's `batchMaxSpend`) are this. */
export function spendCeiling(runs: number, capUsd: number): number {
  return runs * capUsd;
}

/** The most a batch could have spent: the per-run cap times every attempt, the planned cells
 * plus any retries, since each retry had its own cap ("$5.99 of $5.00 max" for one cell that
 * took four attempts read as over budget). */
export function batchMaxSpend(capUsd: number | null | undefined, plannedCells: number, retriedAttempts: number): number | null {
  if (capUsd == null || plannedCells <= 0) return null;
  return spendCeiling(plannedCells + Math.max(0, retriedAttempts), capUsd);
}

/** The batch total, `$4.10 of $12.00 max` when the maximum is known. */
export function batchCostText(totalCostUsd: number, maxSpendUsd: number | null): string {
  return maxSpendUsd != null ? `${usd(totalCostUsd)} of ${usd(maxSpendUsd)} max` : usd(totalCostUsd);
}

/** The batch total's tooltip: what it sums, and how its maximum was counted when retries add
 * attempts beyond the planned cells. */
export function batchCostTitle(plannedCells: number, retriedAttempts: number): string {
  if (retriedAttempts <= 0) return 'summed over every run';
  const attempts = plannedCells + retriedAttempts;
  const cells = `${plannedCells} cell${plannedCells === 1 ? '' : 's'}`;
  return `summed over every attempt, the ${retriedAttempts} retried included; the max is the per-run cap × ${attempts} attempts (${cells} + ${retriedAttempts} retried)`;
}

/** `cap $6.00 / 45m / 24 steps`, the limits each run had; `null` without recorded limits. */
export function limitsText(limits: BatchDetail['limits']): string | null {
  if (!limits) return null;
  const parts = [
    limits.maxCostUsd != null ? `cap ${usd(limits.maxCostUsd)}` : null,
    limits.maxWallMinutes != null ? `${limits.maxWallMinutes}m` : null,
    limits.maxSteps != null ? `${limits.maxSteps} steps` : null,
  ];
  return parts.filter(Boolean).join(' / ');
}

/** The start time as the meta line shows it (`—` when unknown), with the full time as its title. */
export function startedAtText(startedAt: number | null | undefined, now: number): { text: string; title: string } {
  if (!startedAt) return { text: '—', title: '' };
  return { text: timeOfDay(startedAt, now), title: fullTime(startedAt, now) };
}

/** Every run of the batch is a replay: its columns are repetitions of one replay, not seeds. */
export function isReplayBatch(detail: Pick<BatchDetail, 'replays'> & { cells: Pick<BenchmarkCell, 'attempts'>[] }): boolean {
  const replays = detail.replays ?? [];
  const replayRuns = replays.reduce((n, r) => n + r.runs, 0);
  const attempts = detail.cells.reduce((n, c) => n + Math.max(1, c.attempts), 0);
  return replays.length > 0 && replayRuns >= attempts;
}

export type BatchCounts = {
  /** Cells per state group (`cell-states.ts`). */
  byGroup: Record<CellGroup, number>;
  /** Scored cells whose harness was cut short (they are in the mean). */
  cutShort: number;
  /** Every task × seed the batch planned. */
  planned: number;
};

export function batchCounts(detail: Pick<BatchDetail, 'cells' | 'tasks' | 'seeds'>): BatchCounts {
  return {
    byGroup: countByGroup(detail.cells),
    cutShort: detail.cells.filter((c) => c.state === 'scored' && cutShortBadge(c) != null).length,
    planned: detail.tasks.length * detail.seeds.length,
  };
}

/** The batch page's state before pb has any run to show: a `starting` answer from the server, a
 * 404 for a batch this tab started moments ago (still starting as far as anyone knows), or
 * pb's runner having died before it wrote the batch (`failed_to_start`). */
export function batchStartPhase(detail: Pick<BatchDetail, 'status' | 'starting'> | undefined, errorStatus: number | undefined, id: string, now = Date.now()): 'starting' | 'failed_to_start' | null {
  if (detail?.status === 'failed_to_start') return 'failed_to_start';
  if (detail?.status === 'starting') return 'starting';
  if (!detail && errorStatus === 404 && startedRecently(id, now) != null) return 'starting';
  return null;
}
