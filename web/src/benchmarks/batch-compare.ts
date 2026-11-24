// Comparing two batches: the noise verdict per task, its colour, and the verdict over all tasks.
import type { NoiseVerdict, TaskCompareRow } from '../types';
import { duration, usd } from '../format';

/** The server's noise verdict, in words, as pb's own compare says it, with "needs 2 seeds"
 * standing in for pb's "unknown (need ≥2 seeds)". */
export function noiseText(noise: NoiseVerdict): string {
  switch (noise) {
    case 'within_noise':
      return 'within noise';
    case 'beyond_spread':
      return 'beyond seed spread';
    case 'needs_seeds':
      return 'needs 2 seeds';
    case 'n_a':
      return '—';
    default:
      return noise;
  }
}

/** Red only for failed: a real change (`beyond_spread`) is the only verdict worth drawing the
 * eye to; "needs 2 seeds" and "n/a" are muted, not alarming. */
export function noiseTone(noise: NoiseVerdict): 'ok' | 'muted' {
  return noise === 'beyond_spread' ? 'ok' : 'muted';
}

/** Which way a real (beyond-spread) Δ points; `null` (no colour) for anything within noise,
 * needing seeds or not comparable, however large its own number. */
export function deltaTone(noise: NoiseVerdict, delta: number | null | undefined): 'pos' | 'neg' | null {
  if (noise !== 'beyond_spread' || delta == null || delta === 0) return null;
  return delta > 0 ? 'pos' : 'neg';
}

/** The class a tone colours with: green for `pos`, red for `neg`, `neutral` (muted by default)
 * for no tone. The one mapping for every compare and sweep table. */
export function signalClass(tone: 'pos' | 'neg' | null, neutral = 'muted'): string {
  if (tone === 'pos') return 'bm-signal-pos';
  if (tone === 'neg') return 'bm-signal-neg';
  return neutral;
}

/** How the compared tasks' verdicts add up. */
export type VerdictCounts = {
  /** Beyond seed spread, B ahead / B behind. */
  up: number;
  down: number;
  within: number;
  needsSeeds: number;
  /** Tasks both sides ran (`n_a` rows are left out). */
  compared: number;
};

export type OverallVerdict = VerdictCounts & {
  /** `beyond_spread` only when every compared task moved beyond its seed spread, the same way
   * (and none still needs seeds); `mixed` when some did and some did not. */
  verdict: NoiseVerdict | 'mixed';
  /** One line: how many tasks moved beyond spread, which way, and how many are noise. */
  text: string;
};

/** The "Mean over N task(s)" row's own verdict, counted per task instead of letting one task's
 * word speak for all of them. */
export function totalsVerdict(tasks: Pick<TaskCompareRow, 'noise' | 'deltaScore'>[]): OverallVerdict {
  const counts = countVerdicts(tasks);
  return { ...counts, verdict: verdictOf(counts), text: verdictText(counts) };
}

function countVerdicts(tasks: Pick<TaskCompareRow, 'noise' | 'deltaScore'>[]): VerdictCounts {
  const used = tasks.filter((t) => t.noise !== 'n_a');
  const beyond = used.filter((t) => t.noise === 'beyond_spread');
  return {
    up: beyond.filter((t) => (t.deltaScore ?? 0) > 0).length,
    down: beyond.filter((t) => (t.deltaScore ?? 0) < 0).length,
    within: used.filter((t) => t.noise === 'within_noise').length,
    needsSeeds: used.filter((t) => t.noise === 'needs_seeds').length,
    compared: used.length,
  };
}

/** The rule: a real change only when every compared task moved beyond spread in the same
 * direction; never settled while every task, or every unmoved one, still needs seeds. */
export function verdictOf(c: VerdictCounts): OverallVerdict['verdict'] {
  const moved = c.up + c.down;
  if (c.compared === 0) return 'n_a';
  if (c.needsSeeds === c.compared) return 'needs_seeds';
  if (moved === c.compared && (c.up === 0 || c.down === 0)) return 'beyond_spread';
  if (moved === 0) return c.needsSeeds > 0 ? 'needs_seeds' : 'within_noise';
  return 'mixed';
}

/** The words: "1 of 3 beyond spread (B ahead), 2 within noise", "beyond seed spread on all 3
 * tasks (B ahead)", "both tasks need 2 seeds". */
export function verdictText(c: VerdictCounts): string {
  if (c.compared === 0) return '—';
  return [movedText(c), withinText(c), needsSeedsText(c)].filter(Boolean).join(', ');
}

/** "2 of 3", "both tasks", "all 3 tasks"; empty for a single task (its sentence needs no count). */
function share(n: number, compared: number): string {
  if (compared === 1) return '';
  if (n < compared) return `${n} of ${compared}`;
  return compared === 2 ? 'both tasks' : `all ${compared} tasks`;
}

function movedText({ up, down, compared }: VerdictCounts): string | null {
  const moved = up + down;
  if (moved === 0) return null;
  let where: string;
  if (compared === 1) where = 'beyond seed spread';
  else if (moved === compared) where = `beyond seed spread on ${share(moved, compared)}`;
  else where = `${share(moved, compared)} beyond spread`;
  return `${where} (${direction(up, down)})`;
}

function direction(up: number, down: number): string {
  if (up && down) return `B ahead on ${up}, behind on ${down}`;
  return up ? 'B ahead' : 'B behind';
}

function withinText({ within, compared }: VerdictCounts): string | null {
  if (within === 0) return null;
  if (within < compared) return `${within} within noise`;
  return compared === 1 ? 'within noise' : `within noise on ${share(within, compared)}`;
}

function needsSeedsText({ needsSeeds: n, compared }: VerdictCounts): string | null {
  if (n === 0) return null;
  if (n < compared) return `${n} need${n === 1 ? 's' : ''} 2 seeds`;
  return compared === 1 ? 'needs 2 seeds' : `${share(n, compared)} need 2 seeds`;
}

function sign(v: number): string {
  return v > 0 ? '+' : v < 0 ? '-' : '';
}

export function signedPp(v: number | null | undefined, digits = 1): string {
  return v == null ? '—' : `${sign(v)}${Math.abs(v * 100).toFixed(digits)}pp`;
}

export function signedUsd(v: number | null | undefined): string {
  return v == null ? '—' : `${sign(v)}${usd(Math.abs(v))}`;
}

export function signedDuration(seconds: number | null | undefined): string {
  return seconds == null ? '—' : `${sign(seconds)}${durationS(Math.abs(seconds))}`;
}

/** A time pb reports in seconds, as `duration` words it; `—` when unknown. */
export function durationS(seconds: number | null | undefined): string {
  return seconds == null ? '—' : duration(seconds * 1000);
}
