// A batch cell's state, in one table: its word, its colour and the group the batch page counts
// it in. The colour rule is DESIGN.md's (UI): red only for failed, amber for a limit, grey for a
// Stop, blue for running, no colour for the expected outcome.
import type { BenchmarkCell, BenchmarkCellState } from '../types';
import type { BenchmarkCellState as KnownCellState } from '../generated/shapes';

export type CellTone = 'running' | 'bad' | 'warn' | 'muted' | 'ok' | null;

/** The groups the batch page counts cells in (its summary line and its legend). */
export type CellGroup = 'scored' | 'running' | 'queued' | 'failed' | 'limit' | 'interrupted' | 'notStarted';

type CellStateRow = {
  /** The word in the cell and its tooltip. */
  label: string;
  /** `failed: <reason>` / `crashed: <reason>` when pb recorded a reason. */
  withReason?: boolean;
  tone: CellTone;
  group: CellGroup;
};

/** `interrupted` is an operator's own Stop: grey, not a failure (the Stop dialog says it "marks
 * them interrupted", so the cell says the same). A limit (`timeout`, `over_budget`, `over_steps`)
 * is amber, not a crash. Only `failed` and `crashed` are real failures. */
export const CELL_STATES: Record<KnownCellState, CellStateRow> = {
  queued: { label: 'queued', tone: 'muted', group: 'queued' },
  not_started: { label: 'not started', tone: 'muted', group: 'notStarted' },
  running_agent: { label: 'running agent', tone: 'running', group: 'running' },
  evaluating: { label: 'evaluating', tone: null, group: 'running' },
  scored: { label: 'scored', tone: null, group: 'scored' },
  interrupted: { label: 'interrupted', tone: 'muted', group: 'interrupted' },
  timeout: { label: 'over time limit', tone: 'warn', group: 'limit' },
  over_budget: { label: 'over budget', tone: 'warn', group: 'limit' },
  over_steps: { label: 'over step limit', tone: 'warn', group: 'limit' },
  crashed: { label: 'crashed', withReason: true, tone: 'bad', group: 'failed' },
  failed: { label: 'failed', withReason: true, tone: 'bad', group: 'failed' },
};

/** The row for a state, or `undefined` for a state a newer pb added (shown as pb wrote it). */
function stateRow(state: BenchmarkCellState): CellStateRow | undefined {
  return (CELL_STATES as Partial<Record<string, CellStateRow>>)[state];
}

/** One word (or `failed: <reason>` / `crashed: <reason>`) for a cell's state. */
export function stateLabel(state: BenchmarkCellState, exitReason?: string | null): string {
  const row = stateRow(state);
  if (!row) return state;
  return row.withReason && exitReason ? `${row.label}: ${exitReason}` : row.label;
}

/** The cell's colour. Uses `BATCH_STATUS_TONE`'s vocabulary, so a cell and the batch pill it
 * sits under use the same colour for the same thing. */
export function cellStateTone(state: BenchmarkCellState): CellTone {
  return stateRow(state)?.tone ?? null;
}

export function cellGroup(state: BenchmarkCellState): CellGroup | null {
  return stateRow(state)?.group ?? null;
}

/** The legend's colour key, in reading order: one item per group (or pair of groups), drawn in
 * the class the matrix gives those cells. Every group but `scored` appears once. */
export const STATE_LEGEND: { groups: CellGroup[]; text: string; className: string }[] = [
  { groups: ['running'], text: 'running', className: 'tone-text-running' },
  { groups: ['failed'], text: 'failed / crashed', className: 'tone-text-bad' },
  { groups: ['limit'], text: 'over a limit, no score', className: 'tone-text-warn' },
  { groups: ['interrupted', 'notStarted'], text: 'interrupted / not started', className: 'muted' },
  { groups: ['queued'], text: 'queued', className: 'bm-cell-queued' },
];

/** How many cells are in each group. A state a newer pb added counts in none. */
export function countByGroup(cells: Pick<BenchmarkCell, 'state'>[]): Record<CellGroup, number> {
  const counts: Record<CellGroup, number> = { scored: 0, running: 0, queued: 0, failed: 0, limit: 0, interrupted: 0, notStarted: 0 };
  for (const cell of cells) {
    const group = cellGroup(cell.state);
    if (group) counts[group] += 1;
  }
  return counts;
}

/** The words for each unfinished group in the batch summary, in the order the summary lists them. */
const SUMMARY_WORDS: [Exclude<CellGroup, 'scored'>, string][] = [
  ['running', 'running'],
  ['queued', 'queued'],
  ['failed', 'failed'],
  ['limit', 'over a limit'],
  ['interrupted', 'interrupted'],
  ['notStarted', 'not started'],
];

/** The batch summary: `3/6 scored (1 cut short, in the mean), 2 running, 1 failed`. Only the
 * groups that have a cell are named. */
export function summaryText(counts: Record<CellGroup, number>, plannedCells: number, cutShortCount: number): string {
  let text = `${counts.scored}/${plannedCells} scored`;
  if (cutShortCount > 0) text += ` (${cutShortCount} cut short, in the mean)`;
  for (const [group, word] of SUMMARY_WORDS) {
    if (counts[group] > 0) text += `, ${counts[group]} ${word}`;
  }
  return text;
}
