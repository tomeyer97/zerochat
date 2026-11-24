// Formatters for what the pages show, in three groups: time, tokens and cost, text.
//
// Locales: dates and times use the viewer's locale (`undefined`); the thousands separators in
// money and token counts are always `en-US` (the goldens pin both).
import type { Usage } from './types';

// ---------- time ----------

/**
 * `950ms`, `42s`, `3m 05s`, `2h 14m`, `3d 4h`. With `milliseconds: false`, under a second is
 * `0s` (for lists that count in whole seconds). From an hour up, seconds are never shown.
 */
export function duration(ms: number | null | undefined, { milliseconds = true } = {}): string {
  if (ms == null || !Number.isFinite(ms)) return '—';
  if (ms < 0) ms = 0;
  if (ms < 1000) return milliseconds ? `${Math.round(ms)}ms` : '0s';
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, '0')}s`;
  const h = Math.floor(m / 60);
  if (h < 48) return `${h}h ${String(m % 60).padStart(2, '0')}m`;
  const d = Math.floor(h / 24);
  return `${d}d ${h % 24}h`;
}

/**
 * How long something has taken: while it is `live`, from `startedAt` until `now`; otherwise (or
 * when its start is unknown) the `recorded` time, if any. The one rule behind every elapsed time
 * on screen (run list, graph nodes, node panel, run header).
 */
export function elapsedMs({ live, startedAt, recorded }: { live: boolean; startedAt?: number | null; recorded?: number | null }, now: number): number | null {
  return live && startedAt ? now - startedAt : (recorded ?? null);
}

/** `+0s`, `+48s`, `+1m 05s`: how far into the run, always in whole seconds (never `+0ms`
 * alongside `+10s`). */
export function offsetText(at: number | null | undefined, start: number | null | undefined): string {
  if (at == null || start == null) return '';
  const s = Math.floor(Math.max(0, at - start) / 1000);
  return `+${duration(s * 1000, { milliseconds: false })}`;
}

/** `just now`, `5m ago`, `3h ago`, `2d ago`, else a date. */
export function relativeAge(at: number | null | undefined, now: number): string {
  if (at == null) return '';
  const diff = now - at;
  if (diff < 45_000) return 'just now';
  const m = Math.round(diff / 60_000);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(diff / 3_600_000);
  if (h < 36) return `${h}h ago`;
  const d = Math.round(diff / 86_400_000);
  if (d < 14) return `${d}d ago`;
  return new Date(at).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

/** `in 8 min`, `in 2h 05m`, or `now` once it has passed. */
export function untilText(at: number, now: number): string {
  const ms = at - now;
  if (ms <= 30_000) return 'now';
  if (ms < 3_600_000) return `in ${Math.ceil(ms / 60_000)} min`;
  // An hour or more: `duration` already leaves the seconds out.
  return `in ${duration(ms)}`;
}

/** `12:24:36 PM`, or with the date: the one full format, `Nov 4, 12:24:36 PM` (see `fullTime`). */
export function clockTime(at: number | null | undefined, withDate = false): string {
  if (at == null) return '—';
  if (withDate) return fullTime(at);
  return new Date(at).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit', second: '2-digit' });
}

/**
 * When, in a list or a line: `2:21 PM` today, `Oct 28, 2:21 PM` this year, `Oct 28, 2025` before.
 * The exact time goes in a tooltip (`fullTime`).
 */
export function timeOfDay(at: number | null | undefined, now: number = Date.now()): string {
  if (at == null) return '—';
  const date = new Date(at);
  const today = new Date(now);
  if (date.toDateString() === today.toDateString()) {
    return date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  }
  if (date.getFullYear() !== today.getFullYear()) {
    return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
  }
  return date.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
}

/**
 * The one exact format, for tooltips and Details: `Nov 4, 12:24:36 PM`, with the year when it is
 * not this year (`Oct 28, 2025, 2:21:01 PM`).
 */
export function fullTime(at: number | null | undefined, now: number = Date.now()): string {
  if (at == null) return '—';
  const date = new Date(at);
  const sameYear = date.getFullYear() === new Date(now).getFullYear();
  return date.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    ...(sameYear ? {} : { year: 'numeric' }),
    hour: 'numeric',
    minute: '2-digit',
    second: '2-digit',
  });
}

// ---------- tokens and cost ----------

/** `1.2k`, `3.4M`. */
export function compact(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return '—';
  const abs = Math.abs(n);
  if (abs < 1000) return String(Math.round(n));
  if (abs < 1_000_000) return `${trim(n / 1000)}k`;
  if (abs < 1_000_000_000) return `${trim(n / 1_000_000)}M`;
  return `${trim(n / 1_000_000_000)}B`;
}

function trim(value: number) {
  const fixed = Math.abs(value) < 10 ? value.toFixed(1) : value.toFixed(0);
  return fixed.endsWith('.0') ? fixed.slice(0, -2) : fixed;
}

export function totalTokens(usage: Usage | null | undefined): number {
  if (!usage) return 0;
  return (usage.inputTokens || 0) + (usage.outputTokens || 0) + (usage.cacheReadInputTokens || 0) + (usage.cacheCreationInputTokens || 0);
}

/**
 * `339k tokens` and, when most of them are cache reads, `330k cached` (left out only when the
 * exact counts are equal, not merely when they round to the same compact string).
 */
export function tokenParts(usage: Usage | null | undefined): { total: string; cached: string | null } {
  const total = totalTokens(usage);
  const cached = usage?.cacheReadInputTokens ?? 0;
  const show = total > 0 && cached / total >= 0.5 && cached !== total;
  return { total: `${compact(total)} tokens`, cached: show ? `${compact(cached)} cached` : null };
}

/** One line per token kind, for tooltips. */
export function usageBreakdown(usage: Usage | null | undefined): string {
  if (!usage) return 'No token usage recorded.';
  const n = (v: number) => (v || 0).toLocaleString('en-US');
  return [
    `Input: ${n(usage.inputTokens)}`,
    `Output: ${n(usage.outputTokens)}`,
    `Cache read: ${n(usage.cacheReadInputTokens)}`,
    `Cache write: ${n(usage.cacheCreationInputTokens)}`,
    `Total: ${n(totalTokens(usage))}`,
  ].join('\n');
}

/** `$0.04`, `$1.23`, `$17.40`, `<$0.01`. */
export function usd(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  if (value === 0) return '$0.00';
  if (value < 0.01) return '<$0.01';
  if (value < 1000) return `$${value.toFixed(2)}`;
  return `$${Math.round(value).toLocaleString('en-US')}`;
}

/** What `~$` means, said once, in the cost tooltip. */
export const LIST_PRICE_NOTE = '~$ is a list-price equivalent estimated from token counts, not necessarily money spent (e.g. on a subscription login).';

/** `~$0.07` for estimates, `$0.07` for provider-reported costs; `—` for an estimate of nothing. */
export function costText(value: number | null | undefined, estimated: boolean): string {
  if (value == null || (estimated && value === 0)) return '—';
  return `${estimated ? '~' : ''}${usd(value)}`;
}

// ---------- text ----------

export function shortId(id: string | null | undefined, length = 8): string {
  if (!id) return '';
  return id.length > length ? id.slice(0, length) : id;
}

/** `claude-haiku-4-5-20251001` → `haiku-4-5`; `claude-sonnet-5` → `sonnet-5`: the short name graph
 * cards show (projection.rs `short_model` is its twin; contract.test.ts checks the two agree). Failure
 * copy, Details, headers and every tooltip show the exact id (`laneText`). */
export function shortModel(model: string | null | undefined): string {
  if (!model) return '';
  return model.replace(/^claude-/, '').replace(/-\d{8}$/, '');
}

/** A lane: the agent CLI (harness), its provider and model, each as Zeroshot names it. */
export type LaneParts = { harness?: string | null; provider?: string | null; model?: string | null };

/** `claude · anthropic · claude-haiku-4-5`: exact ids; a missing harness or provider is
 * `unknown` (never a guess), a missing model is left out. */
export function laneText(lane: LaneParts): string {
  return [lane.harness || 'unknown', lane.provider || 'unknown', lane.model].filter(Boolean).join(' · ');
}

/** The run header's lane line: the one lane, or `mixed lanes` with each lane and its nodes in
 * the tooltip; null without any. */
export function lanesLine(lanes: ReadonlyArray<LaneParts & { nodes: string[] }> | null | undefined): { text: string; title: string } | null {
  if (!lanes?.length) return null;
  const title = 'Agent CLI (harness) · provider · model';
  if (lanes.length === 1) return { text: laneText(lanes[0]), title };
  return { text: 'mixed lanes', title: [`${title}:`, ...lanes.map((l) => `${l.nodes.join(', ')}: ${laneText(l)}`)].join('\n') };
}

export function plural(n: number, one: string, many = `${one}s`) {
  return `${n} ${n === 1 ? one : many}`;
}

export function truncate(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, Math.max(0, max - 1))}…` : text;
}

/** Pretty JSON that never throws. */
export function prettyJson(value: unknown): string {
  try {
    const text = JSON.stringify(value, null, 2);
    return text === undefined ? String(value) : text;
  } catch {
    return String(value);
  }
}
