// The run list (Home and the run page's sidebar): order, status groups, search, the row's
// subtitle, and how often the list is polled.
import type { Overview, RunListing, TargetOverview } from './types';
import { clockTime, duration as durationText, elapsedMs, timeOfDay } from './format';
import { isLive, listingStatus, runStatusInfo, type RunStatus } from './run-status';

/** When a run started, as its row shows it: its first execution, else its creation time. */
export function runStarted(run: RunListing): number | null {
  return run.summary?.startedAt ?? run.createdAt;
}

/** Newest first by the Started time the row shows (a pb run imported today that started on
 * Nov 4 sorts with Nov 4, not by its import-time id). */
export function sortRuns<T extends RunListing>(runs: T[]): T[] {
  return [...runs].sort((a, b) => (runStarted(b) ?? 0) - (runStarted(a) ?? 0) || b.runId.localeCompare(a.runId));
}

/**
 * Whether a run's row still counts up: a live status with no outcome yet. A run whose outcome is
 * in (`summary.endedAt`) stops, even while Zeroshot is still closing it (phase `running`).
 */
export function rowLive(run: RunListing): boolean {
  return isLive(listingStatus(run)) && run.summary?.endedAt == null;
}

/** Local first (the server already orders targets that way); keeps the server order otherwise. */
export function orderTargets(targets: TargetOverview[]): TargetOverview[] {
  return [...targets].sort((a, b) => Number(b.kind === 'local') - Number(a.kind === 'local'));
}

/** Status groups for the run table's filter chips; `failed` includes stalled runs (both need you). */
export type StatusGroup = 'running' | 'failed' | 'stopped' | 'succeeded';

export function statusGroup(status: RunStatus): StatusGroup | null {
  if (isLive(status)) return 'running';
  if (status === 'failed' || status === 'stalled') return 'failed';
  if (status === 'stopped' || status === 'suspended') return 'stopped';
  if (status === 'succeeded') return 'succeeded';
  return null;
}

// ---------- search ----------

/**
 * Case-insensitive match on title, id, status, cause, failure reason, failing node, repository
 * and branch, plus the row's own displayed text (the shortened cause, the clock time and the
 * duration it shows), so a search can't miss what's plainly on screen.
 */
export function matchesFilter(run: RunListing, filter: string): boolean {
  const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const haystack = searchText(run).join(' ').toLowerCase();
  return words.every((w) => haystack.includes(w));
}

/** Everything a search over one run row looks at. */
function searchText(run: RunListing): string[] {
  const source = run.source ?? {};
  const status = listingStatus(run);
  const started = runStarted(run);
  const text = (value: unknown) => (typeof value === 'string' ? value : '');
  return [
    run.title,
    run.runId,
    status,
    statusGroup(status),
    run.summary?.cause,
    runSubtitle(run),
    run.summary?.failureClass,
    run.summary?.failureNode,
    run.terminal?.reason,
    text(source.repository),
    text(source.branch),
    started != null ? timeOfDay(started) : '',
    started != null ? clockTime(started, true) : '',
    run.summary?.durationMs != null ? durationText(run.summary.durationMs) : '',
  ].filter((part): part is string => !!part);
}

// ---------- the row's subtitle ----------

/** The server's short cause for a run that was not accepted in time (projection's `explain.rs`). */
const NOT_ACCEPTED_CAUSE = /^not accepted after (\d+) rounds?$/;
/** The server's short cause for a node that timed out (projection's `agent_copy.rs`). */
const TIMED_OUT_CAUSE_PREFIX = 'timed out after ';

/** The two causes long enough to crowd a list row; everything else is already short. */
function shortCause(cause: string): string {
  const rounds = NOT_ACCEPTED_CAUSE.exec(cause);
  if (rounds) return `rejected ×${rounds[1]}`;
  if (cause.startsWith(TIMED_OUT_CAUSE_PREFIX)) return 'timed out';
  return cause;
}

/** The line under a run's title: its cause when it did not succeed; nothing otherwise (the dot says the status). */
export function runSubtitle(run: RunListing): string {
  const status = listingStatus(run);
  const cause = run.summary?.cause;
  // `stopped` would only repeat the hollow dot.
  return cause && !isLive(status) && cause !== runStatusInfo[status].label.toLowerCase() ? shortCause(cause) : '';
}

// ---------- one row of a run list ----------

/** What a run list row shows besides its title, the same on Home's table and in the sidebar. */
export type RunRowFacts = {
  status: RunStatus;
  live: boolean;
  /** When the run started (its creation time until its first execution started). */
  started: number | null;
  /** Elapsed time: ticking from `started` while live, else the recorded duration (to the run's
   * outcome, the same as the run header). */
  ms: number | null;
  /** `runSubtitle`: the cause of a run that did not succeed, else empty. */
  cause: string;
};

export function runRowFacts(run: RunListing, now: number): RunRowFacts {
  const status = listingStatus(run);
  const live = rowLive(run);
  const started = runStarted(run);
  const ms = elapsedMs({ live, startedAt: started, recorded: run.summary?.durationMs }, now);
  return { status, live, started, ms, cause: runSubtitle(run) };
}

/**
 * The sidebar's label for a run: the task it ran when pb or zsf recognises it (a pb run's title is
 * a bare `pb-<id>`), else its title. Home's table shows the title itself.
 */
export function sidebarRunLabel(run: RunListing): string {
  return run.context?.task || run.title;
}

// ---------- Home's table ----------

/** A run in Home's table, which lists every target's runs together. */
export type HomeRow = RunListing & { target: string; targetName: string };

/** A filter chip: one status group, or all runs. */
export type HomeGroup = StatusGroup | 'all';

/**
 * Home's table for a text `filter` and a chip `group`: `all` runs (newest first), the `rows`
 * shown, each chip's `counts` and whether a target column is needed. The counts follow the text
 * filter but not the chip, so they never contradict the rows.
 */
export function homeRows(targets: TargetOverview[], filter: string, group: HomeGroup): { all: HomeRow[]; rows: HomeRow[]; counts: Partial<Record<HomeGroup, number>>; manyTargets: boolean } {
  const all = sortRuns(targets.flatMap((t) => t.runs.map((r) => ({ ...r, target: t.id, targetName: t.name }))));
  const matching = all.filter((r) => matchesFilter(r, filter));
  const counts: Partial<Record<HomeGroup, number>> = { all: matching.length };
  for (const r of matching) {
    const g = statusGroup(listingStatus(r));
    if (g) counts[g] = (counts[g] ?? 0) + 1;
  }
  const rows = group === 'all' ? matching : matching.filter((r) => statusGroup(listingStatus(r)) === group);
  const manyTargets = new Set(all.map((r) => r.target)).size > 1;
  return { all, rows, counts, manyTargets };
}

/**
 * A short id suffix for runs whose titles repeat: the random tail of the UUIDv7 (its head is the
 * creation time, shared by every run started in the same minute).
 */
export function duplicateTitleSuffixes(runs: RunListing[]): Map<string, string> {
  const byTitle = new Map<string, RunListing[]>();
  for (const r of runs) byTitle.set(r.title, [...(byTitle.get(r.title) ?? []), r]);
  const out = new Map<string, string>();
  for (const group of byTitle.values()) if (group.length > 1) for (const r of group) out.set(r.runId, idTail(r.runId));
  return out;
}

export function idTail(id: string): string {
  return id.replace(/-/g, '').slice(-4);
}

// ---------- polling ----------

/**
 * The new overview, except that a run which was readable before and cannot be read now (Zeroshot
 * mid-write) keeps its last good row instead of turning into "Untitled · Unknown".
 */
export function mergeOverview(prev: Overview | undefined, next: Overview): Overview {
  if (!prev) return next;
  const old = new Map(prev.targets.flatMap((t) => t.runs.map((r) => [`${t.id}/${r.runId}`, r] as const)));
  const targets = next.targets.map((t) => (t.status === 'ok' ? { ...t, runs: t.runs.map((r) => keepReadable(old.get(`${t.id}/${r.runId}`), r)) } : t));
  return { ...next, targets };
}

/** `row`, or the `before` row when `row` cannot be read right now and `before` could. */
function keepReadable(before: RunListing | undefined, row: RunListing): RunListing {
  const unreadable = row.phase === 'unavailable' || (!!row.issue && !row.title);
  const beforeReadable = !!before && before.phase !== 'unavailable';
  return unreadable && beforeReadable ? before : row;
}

/** `full`: a page that shows the run list (Home, a run page's sidebar). `slow`: the same, while
 * an open run has its own live event stream. `nav`: a page with no run list (Benchmarks,
 * Features), which only needs the nav (`?view=nav`). */
export type OverviewMode = 'full' | 'slow' | 'nav';

/** Milliseconds between overview polls, by page mode and whether the tab is visible. */
const OVERVIEW_DELAY: Record<OverviewMode, { visible: number; hidden: number }> = {
  full: { visible: 4_000, hidden: 15_000 },
  slow: { visible: 5_000, hidden: 15_000 },
  nav: { visible: 30_000, hidden: 60_000 },
};

/** How long to wait before the next poll. */
export function overviewDelay(mode: OverviewMode, hidden: boolean): number {
  const delay = OVERVIEW_DELAY[mode];
  return hidden ? delay.hidden : delay.visible;
}
