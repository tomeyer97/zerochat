/**
 * A slice and its attempts in words and colours: status, what it waits on, how its runs ended, and
 * the two places a slice's status is put in words (its card in the graph and the sentence at the
 * top of its panel). Every per-status rule of the slice graph and panel is in this file.
 */
import type { AttemptView, SliceStatus, SliceView } from '../types';
import type { Tone } from '../status';
import { costText, duration, timeOfDay } from '../format';
import { verdictLine } from './hook-output';

/**
 * DESIGN colours: red only failed, amber for what needs you (an approval, a pause, an interrupted
 * slice), blue running, merged quiet (muted), pending and skipped grey.
 */
export function sliceTone(status: SliceStatus): Tone {
  switch (status) {
    case 'failed':
      return 'bad';
    case 'launching':
    case 'running':
    case 'integrating':
      return 'running';
    case 'waiting_for_review':
    case 'waiting_for_approval':
    case 'interrupted':
      return 'warn';
    case 'merged':
      return 'ok';
    default:
      return 'muted';
  }
}

/** A slice whose run is done, not merged, waiting for a person: the same words in the pill, its card, the features list and the panel. */
export const REVIEW_WAIT = 'waiting for your review';

/** zsf's merge gate (clean, web build and tests, cargo build and test: several minutes), short and in full. */
export const GATE_WORDS = 'checks before merge';
export const GATE_FULL = 'checks before merge (build + tests)';
/** The agents are done; Zeroshot is still finishing the run (its postmortem) before zsf takes it up. */
export const CLOSING_WORDS = 'Zeroshot finishing the run';

/** A plan's `checkpoint` slice in the UI (a "checkpoint" on a run page is a workspace snapshot). */
export const APPROVAL_SLICE = 'approval slice';
export const APPROVAL_SLICE_HINT = 'approval slice: waits for your review before it merges';

const STATUS_WORDS: Partial<Record<SliceStatus, string>> = {
  waiting_for_review: REVIEW_WAIT,
  waiting_for_approval: 'needs your approval',
  launching: 'starting',
  integrating: GATE_WORDS,
};

export function sliceStatusLabel(status: SliceStatus): string {
  return STATUS_WORDS[status] ?? status;
}

/**
 * Between a finished run and the merge, where zsf is. `gate`: it runs the checks before merge,
 * `since` the run ended (or, for an approval slice, since you approved it). `accept`: Merge
 * worktree, without the gate (the attempt already has its result). `ended`: the run ended and
 * zsf has not taken it up yet; `review` says it waits for your review next, not the checks;
 * `closingSince`: the agents finished then, and Zeroshot is still finishing the run (its
 * postmortem), which zsf waits for; `finishedSince`: Zeroshot finished the run then, and zsf
 * takes it up on its next poll (the hand-off).
 */
export type MergeStep = { step: 'gate'; since: number | null } | { step: 'accept' } | { step: 'ended'; review: boolean; closingSince: number | null; finishedSince: number | null };

/** zsf's poll when the feature does not say (`limits.poll_secs`' default). */
export const DEFAULT_POLL_SECS = 30;
/** Polls a finished run may wait for zsf before the slice says zsf has not taken it up, and why. */
const LATE_AFTER_POLLS = 3;

/** The hand-off has lasted more than a few of zsf's polls: zsf is stopped, frozen or busy elsewhere. */
function handoffLate(finishedSince: number | null, now: number, pollSecs: number): boolean {
  return finishedSince != null && now - finishedSince > LATE_AFTER_POLLS * Math.max(1, pollSecs) * 1000;
}

export function mergeStep(slice: Pick<SliceView, 'status' | 'attempts' | 'approvedAt' | 'checkpoint' | 'approved'>): MergeStep | null {
  const last = slice.attempts[slice.attempts.length - 1];
  if (slice.status === 'integrating') {
    if (last?.result) return { step: 'accept' };
    const times = [last?.endedAt, slice.approvedAt].filter((t): t is number => t != null);
    return { step: 'gate', since: times.length ? Math.max(...times) : null };
  }
  if (slice.status === 'running' && last?.runEnded) {
    const closingSince = last.runClosingSince ?? null;
    return { step: 'ended', review: slice.checkpoint && !slice.approved, closingSince, finishedSince: closingSince == null ? (last.runFinishedAt ?? null) : null };
  }
  return null;
}

/**
 * The slice's state in a few words, the same in its pill and its card: `checks before merge
 * (build + tests) · 2m 05s` while the gate runs, `Zeroshot finishing the run · 12s` while
 * Zeroshot does (its postmortem), `run finished; checks before merge next · 8s` until zsf starts
 * them (`waiting for zsf · 2m 05s` past a few of its polls), else the status's words.
 */
export function sliceStateText(slice: SliceView, now: number, daemonUp: boolean, pollSecs = DEFAULT_POLL_SECS): string {
  const step = mergeStep(slice);
  switch (step?.step) {
    case 'gate':
      return step.since != null ? `${GATE_FULL} · ${gateElapsed(step.since, now)}` : GATE_FULL;
    case 'accept':
      return 'merging without gate';
    case 'ended': {
      const elapsed = step.finishedSince != null ? ` · ${gateElapsed(step.finishedSince, now)}` : '';
      if (!daemonUp) return `run finished; merges on Start${elapsed}`;
      if (step.closingSince != null) return `${CLOSING_WORDS} · ${gateElapsed(step.closingSince, now)}`;
      if (handoffLate(step.finishedSince, now, pollSecs)) return `waiting for zsf${elapsed}`;
      return `run finished; ${step.review ? 'review' : GATE_WORDS} next${elapsed}`;
    }
    default:
      return sliceStatusLabel(slice.status);
  }
}

function gateElapsed(since: number, now: number): string {
  return shortDuration(Math.max(0, now - since));
}

/** The last attempt's outcome while the slice is between its run and the merge (else `null`): its row agrees with the pill. */
export function attemptStepLabel(slice: Pick<SliceView, 'status' | 'attempts' | 'approvedAt' | 'checkpoint' | 'approved'>): string | null {
  const step = mergeStep(slice)?.step;
  return step === 'gate' ? GATE_WORDS : step === 'ended' ? 'run finished' : null;
}

export function isActiveSlice(status: SliceStatus): boolean {
  return status === 'launching' || status === 'running' || status === 'integrating';
}

/** Stopped and waiting for a person to retry, merge by hand or skip it. */
export function isStoppedSlice(status: SliceStatus): boolean {
  return status === 'failed' || status === 'interrupted';
}

/** Needs you: stopped, or an approval slice waiting for your review (or, merged by an older zsf, your approval). */
export function needsAttention(status: SliceStatus): boolean {
  return isStoppedSlice(status) || status === 'waiting_for_review' || status === 'waiting_for_approval';
}

/** The first slice that needs you (the graph starts there when it is too wide to show whole). */
export function attentionSlice(slices: Pick<SliceView, 'id' | 'status'>[]): string | undefined {
  return slices.find((s) => needsAttention(s.status))?.id;
}

/** The slices that come after `id` (that list it as a dependency). */
export function dependentsOf<S extends Pick<SliceView, 'deps'>>(slices: S[], id: string): S[] {
  return slices.filter((s) => s.deps.includes(id));
}

export function list(ids: string[]): string {
  return ids.join(', ');
}

/** `06 waits` / `06, 07 wait`. */
export function waitVerb(ids: string[]): string {
  return `${ids.join(', ')} ${ids.length === 1 ? 'waits' : 'wait'}`;
}

/** `<1s` under a second (a launch that failed at once), else the usual `45s` / `3m 05s`. */
export function shortDuration(ms: number | null | undefined): string {
  if (ms != null && ms >= 0 && ms < 1000) return '<1s';
  return duration(ms);
}

type WaitSlice = Pick<SliceView, 'status' | 'waitsFor' | 'approvalOf' | 'hold' | 'retryQueued'>;

/** What a pending slice waits on, in one short phrase (empty for other states). */
export function waitingText(slice: WaitSlice, statusOf: (id: string) => SliceStatus | undefined = () => undefined): string {
  if (slice.status !== 'pending') return '';
  const queued = slice.retryQueued ? 'retry queued' : '';
  if (slice.waitsFor.length) {
    const blocked = slice.waitsFor.filter((d) => ['failed', 'interrupted'].includes(statusOf(d) ?? ''));
    if (blocked.length) return `blocked by ${blocked.map((d) => `${d} (${statusOf(d)})`).join(', ')}`;
    if (slice.approvalOf.length) {
      const review = slice.approvalOf.every((d) => statusOf(d) === 'waiting_for_review');
      return `waits for your ${review ? 'review' : 'approval'} of ${list(slice.approvalOf)}`;
    }
    return `waits for ${list(slice.waitsFor)}`;
  }
  if (slice.hold) return queued ? `${queued} · ${slice.hold}` : slice.hold;
  return queued || 'ready';
}

/** The last attempt's failure in words: `gate failed`, `conflict`, `launch failed`. */
export function failureWord(slice: Pick<SliceView, 'attempts'>): string {
  const last = slice.attempts[slice.attempts.length - 1];
  if (last?.result && last.result !== 'abandoned') return attemptResult(last.result, false, { current: true }).label;
  return 'failed';
}

/** Why an attempt ran, by zsf's attempt kind (also the first word of a `launched` event). */
export const ATTEMPT_KIND_WORDS: Record<string, string> = {
  first: 'first run',
  'infra-retry': 'automatic retry',
  rerun: 're-run after conflict',
  'manual-retry': 'your retry',
  'gate-rerun': 're-run after gate failed',
};

export function attemptKindLabel(kind: string, fresh = false): string {
  return `${ATTEMPT_KIND_WORDS[kind] ?? kind.replace(/-/g, ' ')}${fresh ? ', from scratch' : ''}`;
}

/** How an attempt ended, by zsf's attempt result. The events of the same name use these words too. */
export const RESULT_WORDS: Record<string, string> = {
  merged: 'merged',
  gate_failed: 'gate failed',
  empty: 'no changes',
  conflict: 'conflict',
  quality_failed: 'review rounds ran out',
  stopped: 'stopped',
  infra_failed: 'run failed',
  launch_failed: 'launch failed',
  integration_failed: 'merge failed',
  skipped: 'skipped',
  changes_requested: 'changes requested',
};

/**
 * An attempt's outcome. Red only for the failure a person must act on now (`current`); a
 * failure that a later attempt already moved past is history and stays muted.
 */
export function attemptResult(result: string | null, running: boolean, opts: { current?: boolean; replacedBy?: number } = {}): { label: string; tone: Tone } {
  if (!result) return running ? { label: 'running', tone: 'running' } : { label: '—', tone: 'muted' };
  if (result === 'abandoned') return { label: opts.replacedBy ? `replaced by #${opts.replacedBy}` : 'stopped', tone: 'muted' };
  if (result === 'merged') return { label: 'merged', tone: 'ok' };
  const label = RESULT_WORDS[result] ?? result.replace(/_/g, ' ');
  // Your review sent it back: history, not a failure.
  const failure = !['skipped', 'stopped', 'changes_requested'].includes(result);
  return { label, tone: failure && opts.current ? 'bad' : 'muted' };
}

/** Time spent on all attempts (each finished run's own time); a running attempt counts up to `now`. */
export function sliceDuration(slice: Pick<SliceView, 'attempts'>, now: number): number | null {
  if (!slice.attempts.length) return null;
  return slice.attempts.reduce((sum, a) => sum + attemptDuration(a, now), 0);
}

export function attemptDuration(a: Pick<AttemptView, 'startedAt' | 'endedAt' | 'runMs'>, now: number): number {
  return a.runMs ?? Math.max(0, (a.endedAt ?? now) - a.startedAt);
}

/**
 * A stop reason, short first: zsf's reasons nest causes with `: ` (`infrastructure: launch failed:
 * the launch exited (…) without printing a run id: zeroshot: … (connection refused)`); the
 * headline is the outermost kind plus the innermost cause.
 */
export function reasonSummary(reason: string | null | undefined): { headline: string; full: string } {
  const full = (reason ?? '').trim();
  const gate = /^gate failed: /.exec(full);
  if (gate) return { headline: `Gate failed: ${verdictLine(full.slice(gate[0].length).replace(/ \/ /g, '\n'))}`, full };
  const parts = full.split(': ').filter(Boolean);
  if (parts.length <= 2) return { headline: capitalize(full), full };
  const kind = parts[0] === 'infrastructure' ? parts[1] : parts[0];
  return { headline: capitalize(`${kind}: ${parts[parts.length - 1]}`), full };
}

export function capitalize(text: string): string {
  return text ? text[0].toUpperCase() + text.slice(1) : text;
}

/** A spec as shown: without its own `# 04: Title` heading and `Depends on:` line (the panel says both). */
export function specBody(text: string, id: string): string {
  const lines = text.split('\n');
  let i = 0;
  while (i < lines.length && !lines[i].trim()) i++;
  const escaped = id.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  if (i < lines.length && new RegExp(`^#+\\s+${escaped}\\b`).test(lines[i])) lines.splice(i, 1);
  return lines
    .filter((l) => !/^\s*\**\s*depends on\s*:/i.test(l))
    .join('\n')
    .replace(/^\s+/, '');
}

/** Comma/space separated ids → unique, trimmed list. */
export function parseDeps(text: string): string[] {
  const out: string[] = [];
  for (const part of text.split(/[\s,]+/)) {
    const id = part.trim();
    if (id && !out.includes(id)) out.push(id);
  }
  return out;
}

// ---------- the card and the panel sentence ----------

export type StatusOf = (id: string) => SliceStatus | undefined;

/** The icon a card shows before its state; the view maps each to a picture. */
export type CardIcon = 'done' | 'approval' | 'failed' | 'interrupted' | 'skipped' | 'running' | null;

/** A slice card's state line: an icon, a few words and, while it runs, the attempt's timer. */
export function sliceCardState(slice: SliceView, now: number, daemonUp: boolean, statusOf: StatusOf, pollSecs = DEFAULT_POLL_SECS): { icon: CardIcon; text: string; timer?: string; wraps?: boolean } {
  const last = slice.attempts[slice.attempts.length - 1];
  switch (slice.status) {
    case 'merged': {
      // The short sha only when there is room (no attempts hint after it).
      const sha = slice.commit && !attemptsHint(slice) ? ` ${slice.commit.slice(0, 7)}` : '';
      return { icon: 'done', text: `merged${sha}${slice.accepted ? ' without gate' : ''}` };
    }
    case 'waiting_for_review':
      return { icon: 'approval', text: REVIEW_WAIT };
    case 'waiting_for_approval':
      return { icon: 'approval', text: 'needs your approval' };
    case 'failed':
      return { icon: 'failed', text: failureWord(slice) };
    case 'interrupted':
      return { icon: 'interrupted', text: 'interrupted' };
    case 'skipped':
      return { icon: 'skipped', text: 'skipped' };
    case 'launching':
    case 'running':
    case 'integrating': {
      const step = mergeStep(slice);
      if (step?.step === 'ended' && step.closingSince != null && daemonUp) {
        return { icon: 'running', text: CLOSING_WORDS, timer: `· ${gateElapsed(step.closingSince, now)}` };
      }
      if (step?.step === 'ended') {
        // The pill's words; the hand-off's elapsed time as the card's timer.
        const [text, elapsed] = sliceStateText(slice, now, daemonUp, pollSecs).split(' · ');
        return { icon: 'done', text, ...(elapsed ? { timer: `· ${elapsed}` } : {}) };
      }
      if (step?.step === 'gate') {
        // The pill's words, the elapsed time as the card's timer.
        return { icon: 'running', text: GATE_FULL, ...(step.since != null ? { timer: `· ${gateElapsed(step.since, now)}` } : {}), wraps: true };
      }
      if (step) return { icon: 'running', text: sliceStateText(slice, now, daemonUp) };
      const which = last && last.n > 1 ? `#${last.n} ${attemptKindLabel(last.kind, last.fresh)}` : sliceStatusLabel(slice.status);
      return { icon: 'running', text: which, timer: last ? shortDuration(attemptDuration(last, now)) : undefined };
    }
    default:
      return { icon: null, text: waitingText(slice, statusOf) };
  }
}

/** Why a slice took more than one attempt, in two words: `re-run (conflict)`, `retried ×2`. */
export function attemptsHint(slice: Pick<SliceView, 'attempts'>): string {
  if (slice.attempts.length < 2) return '';
  const count = (kind: string) => slice.attempts.filter((a) => a.kind === kind).length;
  const times = (n: number) => (n > 1 ? ` ×${n}` : '');
  const parts: string[] = [];
  if (count('rerun')) parts.push('re-run (conflict)');
  const retried = count('manual-retry');
  if (retried) parts.push(`retried${times(retried)}`);
  const autoRetried = count('infra-retry');
  if (autoRetried) parts.push(`auto-retried${times(autoRetried)}`);
  return parts.join(', ');
}

/**
 * A card's one short extra (both are in the panel too): why it took more than one attempt, else
 * the time and cost it took. Nothing while it runs (the state line has the timer) or once skipped.
 */
export function sliceCardMeta(slice: SliceView, now: number): string[] {
  if (isActiveSlice(slice.status)) return [];
  const hint = attemptsHint(slice);
  if (hint) return [hint];
  if (slice.status === 'skipped') return [];
  const meta: string[] = [];
  const spent = sliceDuration(slice, now);
  if (spent) meta.push(shortDuration(spent));
  if (slice.costUsd != null) meta.push(costText(slice.costUsd, slice.costEstimated));
  return meta;
}

/** A card as a screen reader hears it: `04 Title: gate failed; approval slice; after 01, 02`. */
export function sliceCardLabel(slice: SliceView, stateText: string): string {
  const after = slice.deps.length ? `; after ${slice.deps.join(', ')}` : '';
  return `${slice.id} ${slice.title}: ${stateText}${slice.checkpoint ? `; ${APPROVAL_SLICE}` : ''}${after}`;
}

/** `04 starts after it is merged` / `04, 05 start after it is merged` (empty without dependents). */
export function afterMerge(dependents: string[]): string {
  if (!dependents.length) return '';
  return `${dependents.join(', ')} ${dependents.length > 1 ? 'start' : 'starts'} after it is merged`;
}

/** What the panel's sentence needs to know about the feature around the slice. */
export type SentenceContext = { slices: SliceView[]; paused: boolean; daemonUp: boolean; now: number; pollSecs?: number; feature?: string };

/** The panel's one sentence: where the slice stands and what comes next. */
export function sliceStateSentence(slice: SliceView, { slices, paused, daemonUp, now, pollSecs = DEFAULT_POLL_SECS, feature = '<feature>' }: SentenceContext): string {
  const dependents = dependentsOf(slices, slice.id).map((d) => d.id);
  const statusOf: StatusOf = (id) => slices.find((s) => s.id === id)?.status;
  const last = slice.attempts[slice.attempts.length - 1];
  switch (slice.status) {
    case 'merged': {
      const how = slice.accepted ? ' without the gate' : '';
      const at = slice.mergedAt ? ` at ${timeOfDay(slice.mergedAt, now)}` : '';
      const approved = slice.checkpoint && slice.approvedAt ? `; approved ${timeOfDay(slice.approvedAt, now)}` : '';
      return `Merged${how}${at}${approved}.`;
    }
    case 'waiting_for_review': {
      const next = afterMerge(dependents);
      return `${capitalize(REVIEW_WAIT)}; not merged yet.${next ? ` ${next}.` : ''}`;
    }
    case 'waiting_for_approval': {
      const as = slice.commit ? ` as ${slice.commit.slice(0, 7)}` : '';
      const before = dependents.length ? ` before ${dependents.join(', ')} can start` : '';
      return `Merged${as}; waits for your approval${before}.`;
    }
    case 'failed': {
      const waiting = dependents.length ? `; ${waitVerb(dependents)} for it` : '';
      return `Stopped: ${failureWord(slice)}${waiting}.`;
    }
    case 'interrupted':
      return paused ? 'Interrupted by the pause; it continues in its worktree when you unpause.' : 'Interrupted; it continues in its worktree when the feature runs.';
    case 'skipped': {
      const at = slice.skippedAt ? ` at ${timeOfDay(slice.skippedAt, now)}` : '';
      const without = dependents.length ? ` and ${dependents.join(', ')} went ahead without it` : '';
      return `Skipped${at}; its work is not merged${without}.`;
    }
    case 'launching':
      return `Starting attempt ${last?.n ?? 1}.`;
    case 'integrating': {
      const step = mergeStep(slice);
      if (step?.step === 'accept') return 'Merging its worktree without the gate.';
      const since = step?.step === 'gate' && step.since != null ? ` since ${timeOfDay(step.since, now)}` : '';
      return `${capitalize(GATE_FULL)}${since}; it merges when they pass.`;
    }
    case 'running':
      if (last?.runEnded) {
        const step = mergeStep(slice);
        const since = step?.step === 'ended' ? step.finishedSince : null;
        const ago = since != null ? ` ${gateElapsed(since, now)} ago` : '';
        if (!daemonUp) return `Its run finished${ago}, but zsf is not running: it merges when you start the feature (Start, or \`zsf start ${feature}\`).`;
        const next = slice.checkpoint && !slice.approved ? 'your review' : `the ${GATE_FULL}`;
        if (last.runClosingSince != null) return `Its agents finished; Zeroshot is finishing the run (its postmortem), then ${next}.`;
        if (handoffLate(since, now, pollSecs)) {
          return `Its run finished${ago}, and zsf, which looks every ${pollSecs}s, has not taken it up: it may be stuck. Restart it: \`zsf stop ${feature}\`, then \`zsf start ${feature}\`.`;
        }
        const then = slice.checkpoint && !slice.approved ? 'it waits for your review' : `runs the ${GATE_FULL}`;
        return `Its run finished${ago}; zsf takes it up on its next look (every ${pollSecs}s), then ${then}.`;
      }
      return `Running attempt ${last?.n ?? 1} (${attemptKindLabel(last?.kind ?? 'first', last?.fresh)}).`;
    case 'pending': {
      const wait = waitingText(slice, statusOf);
      return wait === 'ready' ? 'Ready: starts next.' : `${capitalize(wait)}.`;
    }
  }
}
