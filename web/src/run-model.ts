/**
 * The rules behind the top of the run page and the node panel: which actions a run offers, what
 * its header and failure card say, and which execution of a node to show. Pure functions on the
 * view; the components in RunHeader.tsx, FailureCard.tsx and NodePanel.tsx only render them.
 *
 * Two tools appear by their short names here, as they do in the API:
 *   zsf  Zeroshot features, the scheduler that runs a feature's slices (`context.kind === 'slice'`).
 *   pb   the ProgramBench runner, the benchmark tool (`costSource === 'pb'`, `context.kind === 'benchmark'`).
 */
import type { Execution, Failure, FailureAction, GraphNodeInfo, Overview, RunView } from './types';
import { failureClass, isLive, isStalled, isStopped, missingData, observeOnly, runStatus } from './status';
import { replayVerbs } from './replay-model';
import { compact, costText, duration, elapsedMs, fullTime, timeOfDay, tokenParts, totalTokens, usageBreakdown } from './format';

// ---------- header ----------

/**
 * A cheap signal for whether the run's workspace changed at all, from the execution snapshots
 * already in the view — no extra fetch just to decide whether the Files button is worth showing.
 * When this Zeroshot doesn't snapshot per node, there is nothing cheap to check, so the button
 * stays (a 401 that never ran is the one case this misses; the drawer still says so).
 */
export function anyFilesChanged(view: Pick<RunView, 'executions' | 'features'>): boolean {
  if (!view.features.workspaceSnapshots) return true;
  return view.executions.some((e) => {
    const base = e.workspaceEnd?.base ?? e.workspaceStart?.commit;
    const end = e.workspaceEnd?.commit;
    return !!base && !!end && base !== end;
  });
}

export function runElapsed(view: Pick<RunView, 'startedAt' | 'endedAt' | 'lastActivityAt' | 'status'>, now: number): number | null {
  const { startedAt, endedAt } = view;
  if (startedAt == null) return null;
  // An ended run stops at its end; one that is no longer live stops at its last activity.
  const recorded = (endedAt ?? view.lastActivityAt ?? now) - startedAt;
  const live = endedAt == null && isLive(runStatus(view));
  return Math.max(0, elapsedMs({ live, startedAt, recorded }, now) ?? 0);
}

/**
 * Whether the header offers Replay: finished runs without a failure card (the card has its own),
 * on a Zeroshot that advertises `replay` (replay with changes). Zeroshot 16.1 does not: it
 * resumes a checkpoint unchanged, which a failed run offers on its failure card.
 */
export function canReplay(view: Pick<RunView, 'status' | 'failure' | 'local' | 'capabilities' | 'context' | 'targetKind'>): boolean {
  // A feature slice's run is scheduled by zsf: a replay would be a run zsf never merges or counts.
  if (view.context?.kind === 'slice') return false;
  // An observe-only source (such as pb's runs): the server refuses actions on it anyway.
  if (observeOnly(view)) return false;
  const status = runStatus(view);
  if (isLive(status) || view.failure) return false;
  if (!(view.capabilities ?? []).includes('replay')) return false;
  return view.local ? view.local.hasCheckpoints : status !== 'unknown';
}

/** The run cost's tooltip. A pb run's cost is pb's own (the benchmark's accounting, what its
 * batch page and cap use); Zeroshot's estimate from the same tokens is named when it differs. */
export function costTitle(view: Pick<RunView, 'costEstimated' | 'costSource' | 'zeroshotCostUsd' | 'zeroshotCostEstimated'>): string {
  if (view.costSource === 'pb') {
    const zs = view.zeroshotCostUsd;
    const other = zs != null ? `\nZeroshot's own estimate: ${costText(zs, view.zeroshotCostEstimated ?? true)} (list prices; the node costs on this page are Zeroshot's)` : '';
    return `pb's recorded cost for this benchmark run, from its pricing table (with the cache-TTL split): the figure its batch, cap and compare use.${other}`;
  }
  return view.costEstimated ? 'Estimated from tokens at API list prices; subscription runs are not billed per token.' : 'Reported by the provider.';
}

/** Run ids before this (2017) are not UUIDv7 timestamps, whatever their first 12 hex digits say. */
const UUID_V7_EARLIEST_MS = 1_500_000_000_000;

/** Unix ms in a UUIDv7 run id. */
export function idTime(id: string): number | null {
  const hex = id.replace(/-/g, '').slice(0, 12);
  const ms = parseInt(hex, 16);
  return /^[0-9a-f]{12}$/i.test(hex) && ms > UUID_V7_EARLIEST_MS ? ms : null;
}

/** A linked run named by when it started (`12:25 PM run`); its title and id in the tooltip. */
export function runLink(overview: Overview | undefined, target: string, runId: string, now: number): { label: string; title: string } {
  const run = overview?.targets.find((t) => t.id === target)?.runs.find((r) => r.runId === runId);
  const at = run?.summary?.startedAt ?? run?.createdAt ?? idTime(runId);
  return { label: at ? `${timeOfDay(at, now)} run` : 'run', title: `${run?.title || 'Run'}\n${runId}` };
}

/** What the run header shows, as plain values. */
export function runHeaderModel(view: RunView, stopping: boolean) {
  const status = runStatus(view);
  const live = isLive(status);
  // Tokens and dollars cover the same scope: the graph's nodes. Background agents are named in
  // the tooltip and in the Log section (Zeroshot does not price them).
  const nodeUsage = view.nodeUsage ?? view.usage;
  const backgroundTokens = view.backgroundUsage ? totalTokens(view.backgroundUsage) : 0;
  // A second, muted figure next to the node tokens: background agents are not priced, but their
  // tokens are real and otherwise invisible outside the Log.
  const background = backgroundTokens > 0 ? compact(backgroundTokens) : null;
  const noTokens = totalTokens(nodeUsage) === 0;
  // A zero estimate (or a zero with nothing counted) says nothing; a reported zero does.
  const showCost = view.costUsd != null && !(view.costUsd === 0 && (view.costEstimated || noTokens));
  const usageTitle = [
    usageBreakdown(nodeUsage),
    background ? `Background agents (not priced): ${background} tokens` : '',
    view.usageLive && live ? "Live from Claude's session log until Zeroshot counts the turn." : '',
  ]
    .filter(Boolean)
    .join('\n');
  const replayOf = view.replayOf?.runId;
  return {
    status,
    shownStatus: stopping && status === 'running' ? ('stopping' as const) : status,
    live,
    observed: observeOnly(view),
    tokens: tokenParts(nodeUsage),
    background,
    noTokens,
    showCost,
    usageTitle,
    missing: missingData(view),
    replayOf,
    hasLineage: !!(replayOf || view.resumedFrom || view.resumedAs || view.replays.length > 0),
    replay: canReplay(view),
    files: anyFilesChanged(view),
  };
}

// ---------- failure card ----------

/** Which node, and which of its settings, the replay dialog opens on. */
export type ReplayFocus = { node: string; field: 'model' | 'timeout' | 'instructions' } | null;

/** With no checkpoint, "Resume" starts a brand new run from the beginning. */
export function resumeLabel(hasCheckpoints: boolean): string {
  return hasCheckpoints ? 'Resume' : 'Start over';
}

/** A fix that changes the model or a timeout needs replay with changes (`replay` capability);
 * Zeroshot 16.1 only resumes a checkpoint unchanged, so it is offered as a plain replay. */
export function effectiveAction(action: FailureAction, capabilities: string[] | undefined): FailureAction {
  return action.startsWith('replay') && !(capabilities ?? []).includes('replay') ? 'replay' : action;
}

/** `canEdit`: this Zeroshot can replay with changes (`replay` capability); otherwise a replay
 * starts again from a checkpoint unchanged, and the button says so (replay-model's `replayVerbs`). */
export function primaryLabel(action: FailureAction, failure: Pick<Failure, 'node'>, hasCheckpoints: boolean, canEdit = false): string {
  switch (action) {
    case 'replay_model':
      return 'Replay with another model…';
    case 'replay_timeout':
      return 'Replay with a longer timeout…';
    case 'replay':
      return replayVerbs(canEdit, failure.node).button;
    case 'resume':
      return resumeLabel(hasCheckpoints);
  }
}

/** Only a model or timeout fix opens that node's settings; a plain replay starts collapsed. */
export function replayFocus(action: FailureAction, node: string | null | undefined): ReplayFocus {
  const field = action === 'replay_model' ? 'model' : action === 'replay_timeout' ? 'timeout' : null;
  return node && field ? { node, field } : null;
}

/** The actions offered next to the primary one: a replay or resume the primary is not already. */
export function secondaryActions(failure: Pick<Failure, 'actions' | 'primary'>): FailureAction[] {
  return (['replay', 'resume'] as const).filter((a) => failure.actions.includes(a) && !(failure.primary === a || (a === 'replay' && failure.primary?.startsWith('replay'))));
}

/** What the failure card says, as plain values. */
export function failureModel(failure: Failure, view: RunView, now: number) {
  const info = failureClass(failure.class);
  const culprit = view.executions.find((e) => e.id === failure.execution);
  const stopped = isStopped(failure);
  const stalled = isStalled(failure);
  const verb = stopped ? 'Force-stopped' : stalled ? 'Last active' : 'Failed';
  const outcome = culprit?.error?.code ? [culprit.error.code, culprit.error.reason].filter(Boolean).join(': ') : null;
  // Two different clocks: how long the node itself ran, and how far into the whole run that
  // was. Label both when they differ meaningfully so neither is mistaken for the other.
  const execMs = culprit?.durationMs;
  const [latest] = failure.feedback;
  return {
    info,
    culprit,
    hasCheckpoints: view.local ? view.local.hasCheckpoints : true,
    waiting: failure.resetsAt != null && failure.resetsAt > now,
    stopped,
    stalled,
    verb,
    /** The title's tooltip: the class, Zeroshot's reason and the node's own outcome code. */
    titleTip: [`${info.label}: ${info.explain}`, `Zeroshot reason: ${failure.reason}`, outcome ? `Node outcome: ${outcome}` : ''].filter(Boolean).join('\n'),
    // The exact timestamp stays a tooltip; the postmortem gap and which nodes finished before it
    // answer "how far into the run" and belong in the text itself, not hidden behind a hover.
    whenTip: failure.at != null ? `${verb} ${fullTime(failure.at, now)}` : undefined,
    execMs,
    intoRunDiffers: execMs != null && failure.intoRunMs != null && Math.abs(failure.intoRunMs - execMs) > 1000,
    // Stopped or stalled: the nodes that were cut off, each linked.
    interrupted: stopped || stalled ? view.executions.filter((e) => e.state === 'interrupted') : [],
    // Only the round that decided it; every round's feedback is already one click away in the
    // node panel's round picker.
    latestRound: latest ? failure.feedback.filter((f) => f.round === latest.round) : [],
    secondary: secondaryActions(failure),
    /** A feature slice's run: its actions live on the slice, in zsf. */
    slice: view.context?.kind === 'slice' && view.context.feature ? view.context : null,
  };
}

// ---------- node panel ----------

/** The executions of a node in order, and which one to show. */
export function nodeExecutions(view: Pick<RunView, 'executions'>, node: string, execution?: number) {
  const list = view.executions.filter((e) => e.node === node);
  const current = (execution != null ? list.find((e) => e.id === execution) : undefined) ?? list[list.length - 1];
  return { list, current };
}

/** The node's tokens for the header, or nothing while Zeroshot has not counted them yet. */
export function usageText(e: Pick<Execution, 'usage' | 'usageSource' | 'state'>): string | null {
  if (e.usageSource === 'pending') return null;
  return tokenParts(e.usage).total;
}

/** One fact in the node panel's header: its text, then an optional link and a muted suffix. */
export type PanelFact = {
  key: string;
  text: string;
  title?: string;
  bad?: boolean;
  link?: { href: string; label: string };
  suffix?: string;
};

/** The node panel header's facts: model, then either "reused" or duration, tokens and cost, then
 * the round and attempt when they are not the first. */
export function nodeFacts({
  current,
  info,
  visits,
  elapsed,
  originalHref,
}: {
  current: Execution;
  info: GraphNodeInfo | undefined;
  /** How many executions the node has (the round picker names the round when there are several). */
  visits: number;
  elapsed: number | null;
  /** Where the original of a reused execution is, when known. */
  originalHref: string | null;
}): PanelFact[] {
  const model = current.model ?? info?.model;
  const facts: PanelFact[] = [{ key: 'm', title: model ?? '', text: `${model || 'no model'}${info?.effort ? ` · ${info.effort}` : ''}` }];
  if (current.reused) {
    facts.push({ key: 'r', text: 'reused', link: originalHref ? { href: originalHref, label: 'open original ›' } : undefined });
  } else {
    // A node that failed on its timeout already shows that duration here; "of Ns" would just
    // repeat it, so only a still-running (or otherwise-failed) node shows the ceiling.
    const timedOut = current.state === 'failed' && current.error?.code === 'timeout';
    facts.push({
      key: 't',
      text: `${current.timesApproximate && elapsed != null ? '≈' : ''}${duration(elapsed)}`,
      title: current.timesApproximate ? 'Approximate: inferred from log timestamps' : '',
      bad: timedOut,
      suffix: timedOut ? ' (limit)' : info?.timeoutMs != null ? ` of ${duration(info.timeoutMs)}` : undefined,
    });
    const tokens = usageText(current);
    // "Running" already says it's live; the count itself doesn't need its own live word too.
    const liveNote = current.usageSource === 'session_log' && current.state === 'running' ? '\nLive from the session log until Zeroshot counts the turn.' : '';
    if (tokens) facts.push({ key: 'k', text: tokens, title: `${usageBreakdown(current.usage)}${liveNote}` });
    if (current.costUsd != null) facts.push({ key: 'c', text: costText(current.costUsd, current.costSource === 'estimate') });
  }
  // The round picker names the round; a first attempt is the normal case.
  if (visits <= 1 && current.round > 1) facts.push({ key: 'o', text: `round ${current.round}` });
  if (current.attempt > 1) facts.push({ key: 'a', text: `attempt ${current.attempt}${info?.attempts ? ` of ${info.attempts}` : ''}` });
  return facts;
}

/** The node's enclosing loop (the innermost) and how many rounds it allows. */
export function nodeLoop(view: Pick<RunView, 'graphNodes'>, info: GraphNodeInfo | undefined): { name: string; max: number | null | undefined } | null {
  const name = info?.loops[info.loops.length - 1];
  return name ? { name, max: view.graphNodes.find((n) => n.name === name)?.maxIterations } : null;
}

// ---------- harnesses ----------

/** The harness whose session log a native transcript was read from, as users name it. */
export function harnessLabel(harness: string | null | undefined): string {
  return harness === 'copilot' ? 'Copilot' : 'Claude';
}

/** Copilot's AI credits in dollars: one credit is one cent. */
export function copilotCreditsUsd(credits: number): number {
  return credits / 100;
}
