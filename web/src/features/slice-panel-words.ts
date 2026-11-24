/**
 * The slice panel's rules that are not about one status: the confirmations of Skip and Merge
 * worktree, the edit form's checks, which reason an attempt row repeats, how an artifact shows,
 * and the mark after a dependency.
 */
import type { SliceStatus, SliceView } from '../types';
import type { Tone } from '../status';
import { statText } from './diff-text';
import { verdictLine } from './hook-output';
import { isActiveSlice, isStoppedSlice, parseDeps, reasonSummary, sliceStatusLabel, sliceTone } from './slice-words';

/** The Skip dialog's text: what happens to its run, that it never merges, and to what comes after it. */
export function skipConsequences(slice: Pick<SliceView, 'id' | 'status' | 'attempts' | 'worktree' | 'worktreeExists'>, dependents: Pick<SliceView, 'id' | 'title'>[]): string {
  const lastRunEnded = slice.attempts[slice.attempts.length - 1]?.runEnded;
  const run = !isActiveSlice(slice.status) ? '' : lastRunEnded ? 'Its run already finished; that work is discarded. ' : 'Its run is stopped. ';
  const names = dependents.map((d) => `${d.id} ${d.title}`).join(', ');
  const many = dependents.length > 1;
  // A slice waiting for review holds its dependents back until it merges: skipping releases them.
  const stopWaiting = slice.status === 'waiting_for_review' ? ` ${many ? 'stop' : 'stops'} waiting for it and` : '';
  const others = dependents.length ? `; ${names}${stopWaiting} ${many ? 'go' : 'goes'} ahead without it` : '';
  const worktree = slice.worktree && slice.worktreeExists ? ' Its worktree stays where it is.' : '';
  return `${run}${slice.id} is never merged${others}. This can't be undone here.${worktree}`;
}

/** How a review action is offered: at once, in the notes form, or after a confirmation. */
export type ReviewOpens = 'now' | 'form' | 'dialog';
export type ReviewAction = { action: 'approve' | 'retry' | 'skip'; label: string; opens: ReviewOpens };

/** A slice waiting for review offers these three, in this order (Request changes is zsf's retry with notes). */
export const REVIEW_ACTIONS: readonly ReviewAction[] = [
  { action: 'approve', label: 'Approve: run checks and merge', opens: 'now' },
  { action: 'retry', label: 'Request changes…', opens: 'form' },
  { action: 'skip', label: 'Skip…', opens: 'dialog' },
];

/** The review actions of a slice waiting for review, those zsf accepts now; `null` for any other state. */
export function reviewActions(slice: Pick<SliceView, 'status' | 'actions'>): ReviewAction[] | null {
  if (slice.status !== 'waiting_for_review') return null;
  return REVIEW_ACTIONS.filter((a) => slice.actions.includes(a.action));
}

/** Request changes' body: the notes are the review, so blank notes send nothing (`null`). */
export function requestChangesBody(notes: string): { action: 'retry'; notes: string } | null {
  const text = notes.trim();
  return text ? { action: 'retry', notes: text } : null;
}

/** The Merge worktree dialog's text: what it merges, and when to use it. */
export function acceptSummary(slice: Pick<SliceView, 'worktreeChanges'>): string {
  const what = slice.worktreeChanges ? `: ${statText(slice.worktreeChanges)}` : '';
  return `Merges its worktree as it is now${what}. Use this after you finished it by hand; the gate does not run.`;
}

/** What the edit form typed (as typed: `deps` is the comma/space separated text). */
export type EditDraft = { title: string; deps: string; spec: string };
/** The edit action's body: only what changed. */
export type EditBody = { title?: string; deps?: string[]; spec?: string };

/**
 * The edit form's checks and the body it sends. `sliceIds` are the feature's slices. A slice
 * cannot come after itself or after a slice the feature does not have; the title cannot be
 * empty; the spec is compared without trailing blank lines. `mentionsDeps`: the dependencies
 * change but the spec has its own `Depends on:` line, which the agent still reads as written.
 */
export function editDiff(
  slice: Pick<SliceView, 'id' | 'title' | 'deps' | 'spec'>,
  draft: EditDraft,
  sliceIds: string[]
): { titleError: string | null; depsError: string | null; body: EditBody; changed: boolean; mentionsDeps: boolean } {
  const title = draft.title.trim();
  const deps = parseDeps(draft.deps);
  const unknown = deps.filter((d) => !sliceIds.includes(d));
  const depsError = deps.includes(slice.id) ? `${slice.id} can't come after itself` : unknown.length ? `not a slice of this feature: ${unknown.join(', ')}` : null;
  const titleError = title ? null : "the title can't be empty";
  const body: EditBody = {};
  if (title !== slice.title) body.title = title;
  if (deps.join(',') !== slice.deps.join(',')) body.deps = deps;
  if (slice.spec != null && draft.spec.trimEnd() !== slice.spec.trimEnd()) body.spec = draft.spec;
  const mentionsDeps = body.deps !== undefined && /depends on\s*:/i.test(draft.spec);
  return { titleError, depsError, body, changed: Object.keys(body).length > 0, mentionsDeps };
}

/**
 * The reason an attempt row shows under its kind, said once: not when the row above gave the
 * same reason, and not the one "Why it stopped" already shows (the stopped slice's own reason).
 */
export function attemptReason(slice: Pick<SliceView, 'attempts' | 'status' | 'reason'>, i: number): string | null {
  const attempt = slice.attempts[i];
  if (!attempt.reason) return null;
  const headline = attempt.result === 'gate_failed' ? verdictLine(attempt.reason) : reasonSummary(attempt.reason).headline;
  if (!headline) return null;
  if (attempt.reason === slice.attempts[i - 1]?.reason) return null;
  const stopped = isStoppedSlice(slice.status);
  const current = stopped && i === slice.attempts.length - 1;
  if (current && slice.reason) return null;
  const why = stopped && slice.reason ? reasonSummary(slice.reason).headline : null;
  if (headline === why || `Gate failed: ${headline}` === why) return null;
  return headline;
}

/**
 * How an artifact shows: Markdown rendered; a log from its end (where test runners put the
 * failure): `*.log`, `*.txt` and every file under `gate/`, the gate hook's output; anything
 * else as text.
 */
export function artifactKind(path: string): 'markdown' | 'log' | 'text' {
  if (/\.md$/i.test(path)) return 'markdown';
  if (/(^|\/)gate\//i.test(path) || /\.(log|txt)$/i.test(path)) return 'log';
  return 'text';
}

/** The mark after a dependency's id: `✓` once merged, else its status; its tone colours it. */
export function depMark(status: SliceStatus | undefined): { text: string; tone: Tone } {
  if (!status) return { text: '', tone: 'muted' };
  return { text: status === 'merged' ? '✓' : `(${sliceStatusLabel(status)})`, tone: sliceTone(status) };
}
