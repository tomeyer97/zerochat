// One cell of the batch matrix (a task × seed run): its cut-short marker, its tooltip, its
// column header, and the lookups that build the grid.
import type { BenchmarkAggregate, BenchmarkCell, CutShort } from '../types';
import type { CutShort as KnownCutShort } from '../generated/shapes';
import { duration, usd } from '../format';
import { stateLabel } from './cell-states';

type CutCaps = Pick<BenchmarkCell, 'capUsd' | 'capMinutes' | 'capSteps'>;
type CutText = { short: string; long: string };

/** What stopped a scored cell's harness before it finished, or `null` when it finished on its
 * own. An older server sends only `overLimit`; its limit names match `CutShort`'s. */
export function cutReason(cell: Pick<BenchmarkCell, 'cutShort' | 'overLimit'>): CutShort | null {
  if (cell.cutShort != null) return cell.cutShort;
  const limit = cell.overLimit;
  return limit === 'budget' || limit === 'timeout' || limit === 'steps' ? limit : null;
}

/** Per reason: the cell's tag (names the limit hit) and the start of its tooltip sentence. */
const CUT_TEXT: Record<KnownCutShort, (caps: CutCaps) => CutText> = {
  timeout: ({ capMinutes: m }) =>
    m != null ? { short: `⏱ ${trimNum(m)}m`, long: `stopped at the ${trimNum(m)}-minute wall-time limit` } : { short: '⏱ time', long: 'stopped at the wall-time limit' },
  budget: ({ capUsd: cap }) => (cap != null ? { short: `$${trimNum(cap)} cap`, long: `stopped at the ${usd(cap)} budget cap` } : { short: '$ cap', long: 'stopped at the budget cap' }),
  steps: ({ capSteps: n }) => (n != null ? { short: `${n} steps`, long: `stopped at the ${n}-step limit` } : { short: 'step cap', long: 'stopped at the step limit' }),
  interrupted: () => ({ short: 'stopped', long: 'interrupted (a Stop) before it finished' }),
  crashed: () => ({ short: 'crashed', long: 'the harness crashed before it finished' }),
};

/** The visible marker on a scored cell whose harness was cut short (a limit, a Stop, a crash).
 * pb scores the code that existed then (`eval_after_limit`), so the score is real but of partial
 * work, and it is counted in the mean. `short` fits in the cell and names the limit hit; `long`
 * is its tooltip and legend sentence. A reason a newer pb added reads as a crash. */
export function cutShortBadge(cell: Pick<BenchmarkCell, 'cutShort' | 'overLimit' | 'capUsd' | 'capMinutes' | 'capSteps'>): CutText | null {
  const why = cutReason(cell);
  if (why == null) return null;
  const text = (CUT_TEXT as Partial<Record<string, (caps: CutCaps) => CutText>>)[why] ?? CUT_TEXT.crashed;
  const { short, long } = text(cell);
  return { short, long: `${long}, scored on the code it had; counted in the mean` };
}

function trimNum(v: number): string {
  return Number.isInteger(v) ? String(v) : v.toFixed(2).replace(/0+$/, '').replace(/\.$/, '');
}

/** ` of $5.00 cap`: how a cost reads next to its per-run cap (empty without a cap). */
export function capText(capUsd: number | null | undefined): string {
  return capUsd != null ? ` of ${usd(capUsd)} cap` : '';
}

/** The multi-line tooltip for one cell: everything the matrix has no room to show (one place
 * per fact on the page, the detail in a tooltip). */
export function cellTitle(cell: BenchmarkCell, now?: number): string {
  if (cell.state === 'queued') return 'queued: pb starts it as soon as a parallel slot is free';
  if (cell.state === 'not_started') return 'not started: the batch ended before pb got to it';
  const lines = [stateLabel(cell.state, cell.exitReason)];
  if (cell.state === 'running_agent') lines.push(...runningLines(cell, now));
  const badge = cutShortBadge(cell);
  if (badge) lines.push(badge.long);
  lines.push(...resultLines(cell));
  if (cell.attempts > 1) lines.push(attemptLine(cell));
  return lines.join('\n');
}

/** A running cell: how long, which node, what it has spent so far, and whether it can be opened. */
function runningLines(cell: BenchmarkCell, now: number | undefined): string[] {
  const lines: string[] = [];
  if (cell.startedAt != null && now != null) lines.push(`running for ${duration(Math.max(0, now - cell.startedAt))}`);
  if (cell.liveNodes) lines.push(liveNodeText(cell.liveNodes) ?? 'agent finished; pb is wrapping up');
  if (cell.liveCostUsd != null) {
    const unpriced = cell.liveCostUnpriced?.length ? ` (not counting ${cell.liveCostUnpriced.join(', ')}: no price in pb.toml)` : '';
    const estimate = `live estimate from the agent's tokens at pb.toml prices${unpriced}`;
    lines.push(`spent so far ~${usd(cell.liveCostUsd)}${capText(cell.capUsd)}: ${estimate}; pb records the cost when the agent finishes`);
  }
  lines.push(cell.zeroshotRunId ? 'click to watch it live' : 'its Zeroshot run has not started yet');
  return lines;
}

/** What the run produced: tests, regression tests, cost against its cap, wall time. */
function resultLines(cell: BenchmarkCell): string[] {
  const lines: string[] = [];
  if (cell.testsTotal != null) lines.push(`tests ${cell.testsPassed ?? 0}/${cell.testsTotal}`);
  if (cell.regressionTotal != null) lines.push(`regression ${cell.regressionPassed ?? 0}/${cell.regressionTotal}`);
  if (cell.costUsd != null) {
    const overCap = cell.capUsd != null && cell.costUsd > cell.capUsd;
    lines.push(`cost ${usd(cell.costUsd)}${overCap ? `, over the ${usd(cell.capUsd)} cap` : capText(cell.capUsd)}`);
  }
  if (cell.wallS != null) lines.push(`wall time ${duration(cell.wallS * 1000)}`);
  return lines;
}

/** `attempt 3 (earlier: interrupted, crashed: <reason>)`: earlier attempts do not vanish. */
function attemptLine(cell: BenchmarkCell): string {
  const priorWords = cell.priorAttempts.map((p) => stateLabel(p.state, p.exitReason)).join(', ');
  return `attempt ${cell.attempts} (earlier: ${priorWords})`;
}

/** A replay batch's columns are repetitions of one replay, not seeds of a fresh run: "replay 1",
 * "replay 2" (pb numbers them from seed 0). Past 8 columns, the bare number. */
export function seedHeader(seed: number, isReplay: boolean, manySeeds: boolean): { text: string; title: string } {
  if (isReplay) return { text: manySeeds ? String(seed + 1) : `replay ${seed + 1}`, title: `replay ${seed + 1} (pb seed ${seed})` };
  return { text: manySeeds ? String(seed) : `seed ${seed}`, title: `seed ${seed}` };
}

/** `in build`, `in coverage ‖ bugfinder` for parallel nodes, or `null` when no node is running
 * (the agent finished and pb has not recorded it yet). */
export function liveNodeText(nodes: string[] | undefined): string | null {
  if (!nodes || nodes.length === 0) return null;
  return `in ${nodes.join(' ‖ ')}`;
}

/** A map key made of several parts. The parts are joined with NUL, a character no task name,
 * run id or path contains, so two different part lists never give the same key. */
export function compositeKey(...parts: (string | number)[]): string {
  return parts.join('\u0000');
}

export function cellKey(task: string, seed: number): string {
  return compositeKey(task, seed);
}

/** Cells indexed by task+seed, for an O(1) lookup while building the task × seed grid. */
export function cellMap(cells: BenchmarkCell[]): Map<string, BenchmarkCell> {
  const map = new Map<string, BenchmarkCell>();
  for (const cell of cells) map.set(cellKey(cell.task, cell.seed), cell);
  return map;
}

export function aggregateByKey(aggregates: BenchmarkAggregate[]): Map<string, BenchmarkAggregate> {
  return new Map(aggregates.map((a) => [a.key, a]));
}
