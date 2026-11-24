// Scores in words and colour: one score, a mean with its range, and the colour scale.
import type { BenchmarkAggregate } from '../types';

/** Scores closer than this are "the same" (they print alike at two decimals), so a range
 * narrower than it is not shown. */
export const SPREAD_EPSILON = 0.005;

/** Whether a group's scores differ enough to show their range. */
export function hasSpread(min: number | null | undefined, max: number | null | undefined): boolean {
  return min != null && max != null && max - min >= SPREAD_EPSILON;
}

/** A single-hue scale: the score is the only coloured value on the page, so low scores sit
 * closer to `--muted` and high scores closer to `--accent-text`, never a second hue. */
export function scoreColor(score: number): string {
  const pct = Math.round(Math.max(0, Math.min(1, score)) * 75 + 15);
  return `color-mix(in srgb, var(--accent-text) ${pct}%, var(--muted))`;
}

export function scoreText(score: number | null | undefined): string {
  return score == null ? '—' : score.toFixed(2);
}

/** `0.71 (0.58–0.84)`, or just the mean when the scores have no visible spread. */
function meanWithRange(mean: number, min: number | null | undefined, max: number | null | undefined): string {
  if (!hasSpread(min, max)) return scoreText(mean);
  return `${scoreText(mean)} (${scoreText(min)}–${scoreText(max)})`;
}

/** A batch's or a task's aggregate: `0.71 (0.58–0.84)`, the mean alone when every value in the
 * group is equal, or `—` with nothing scored yet. */
export function aggregateText(agg: BenchmarkAggregate | undefined): string {
  if (!agg || agg.n === 0 || agg.mean == null) return '—';
  return meanWithRange(agg.mean, agg.min, agg.max);
}

/** One side of a compared task: `0.71 (0.58–0.84), n=3`, the mean with its count when every
 * scored run had the same score, or `—` when this side has nothing scored. */
export function taskAggText(agg: { n: number; meanScore: number; minScore: number; maxScore: number } | null): string {
  if (!agg || agg.n === 0) return '—';
  return `${meanWithRange(agg.meanScore, agg.minScore, agg.maxScore)}, n=${agg.n}`;
}
