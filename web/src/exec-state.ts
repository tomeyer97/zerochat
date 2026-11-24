// One execution (one visit of a node): its state and how it is labelled, which execution a
// node link names, and the `j`/`k` keys that step through them.
import type { ExecState, Execution, RunView } from './types';
import type { Tone } from './run-status';

export type NodeState = ExecState | 'idle';

export const execStateInfo: Record<NodeState, { label: string; tone: Tone; explain: string }> = {
  idle: { label: 'Not started', tone: 'muted', explain: 'This node has not run (yet).' },
  running: { label: 'Running', tone: 'running', explain: 'The agent is working right now.' },
  succeeded: { label: 'Succeeded', tone: 'ok', explain: 'The node finished and returned a result.' },
  accepted: { label: 'Accepted', tone: 'ok', explain: 'The verifier accepted the work.' },
  rejected: { label: 'Rejected', tone: 'warn', explain: 'The verifier rejected the work and gave feedback.' },
  failed: { label: 'Failed', tone: 'bad', explain: 'The node errored (the agent or its provider failed).' },
  interrupted: {
    label: 'Interrupted',
    tone: 'neutral',
    explain: 'The node was still running when the run stopped (force stop, crash or lost runtime).',
  },
  voided: { label: 'Voided', tone: 'muted', explain: 'Zeroshot discarded this execution (for example after a restart).' },
};

/** The one-character verdict mark of each state, for compact lists. */
const stateMarks: Record<NodeState, string> = {
  idle: '○', // U+25CB white circle
  running: '●', // U+25CF black circle
  succeeded: '✓', // U+2713 check mark
  accepted: '✓',
  rejected: '✗', // U+2717 ballot x
  failed: '✗',
  interrupted: '■', // U+25A0 black square
  voided: '–', // U+2013 en dash
};

export function nodeState(value: string | null | undefined): NodeState {
  return value && value in execStateInfo ? (value as NodeState) : 'idle';
}

/** `✓`, `✗`, … a one-character verdict mark for compact lists (`○` for anything unknown). */
export function stateMark(state: NodeState): string {
  return Object.hasOwn(stateMarks, state) ? stateMarks[state] : stateMarks.idle;
}

/** The verdict of a verifier execution in words: `verdict: rejected`. */
export function verdictText(e: Pick<Execution, 'signals'>): string {
  if (!e.signals) return '';
  return Object.entries(e.signals)
    .map(([k, v]) => `${k}: ${typeof v === 'string' ? v : JSON.stringify(v)}`)
    .join(', ');
}

/** The reviewer's feedback (`diagnostic.message`, or the whole diagnostic as text). */
export function diagnosticText(diagnostic: unknown): string {
  if (diagnostic == null) return '';
  if (typeof diagnostic === 'string') return diagnostic;
  if (typeof diagnostic === 'object' && !Array.isArray(diagnostic)) {
    const d = diagnostic as Record<string, unknown>;
    if (typeof d.message === 'string') return d.message;
  }
  return JSON.stringify(diagnostic, null, 2);
}

/** `Round 2 · attempt 2` style label of one execution among the node's visits. */
export function executionLabel(e: Pick<Execution, 'round' | 'attempt'>, showRound = true): string {
  const parts: string[] = [];
  if (showRound) parts.push(`Round ${e.round}`);
  if (e.attempt > 1) parts.push(`attempt ${e.attempt}`);
  return parts.join(' · ');
}

/** `round 1 of 2 · attempt 2 of 3`, leaving out what does not apply. */
export function positionText(e: Pick<Execution, 'round' | 'attempt'>, maxRounds: number | null | undefined, maxAttempts: number | null | undefined, inLoop: boolean): string {
  const parts: string[] = [];
  if (inLoop) parts.push(`round ${e.round}${maxRounds ? ` of ${maxRounds}` : ''}`);
  if (maxAttempts && maxAttempts > 1) parts.push(`attempt ${e.attempt} of ${maxAttempts}`);
  return parts.join(' · ');
}

/** The execution a node route names (round + attempt, or execution id), and whether it exists. */
export function resolveVisit<E extends { id: number; node: string; round: number; attempt: number }>(
  view: { executions: E[] },
  route: { node?: string; round?: number; attempt?: number; execution?: number }
): { execution?: E; missing: boolean } {
  if (!route.node) return { missing: false };
  const list = view.executions.filter((e) => e.node === route.node);
  if (route.execution != null) {
    const e = list.find((x) => x.id === route.execution);
    return { execution: e, missing: !e };
  }
  if (route.round != null) {
    const inRound = list.filter((x) => x.round === route.round);
    const e = route.attempt != null ? inRound.find((x) => x.attempt === route.attempt) : inRound.at(-1);
    return { execution: e, missing: !e };
  }
  return { execution: list.at(-1), missing: false };
}

type VisitRoute = { node?: string; round?: number; attempt?: number; execution?: number };

/**
 * A node route naming a visit the node never had (a stale link) falls back to the node's latest
 * execution, with a notice saying so. `null` when the visit exists, or the node never ran (the
 * node panel itself says so).
 */
export function visitFallback<E extends { id: number; node: string; round: number; attempt: number }>(view: { executions: E[] }, route: VisitRoute): { fallback: E; notice: string } | null {
  if (!route.node || !resolveVisit(view, route).missing) return null;
  const fallback = view.executions.filter((e) => e.node === route.node).at(-1);
  if (!fallback) return null;
  return { fallback, notice: `${route.node} has no ${visitName(route)} here; showing its latest.` };
}

/** `round 3`, `execution 12`: how the notice names a missing visit. */
function visitName(route: VisitRoute): string {
  if (route.round != null) return `round ${route.round}`;
  if (route.execution != null) return `execution ${route.execution}`;
  return 'that visit';
}

// ---------- keyboard ----------

/** Executions in order; `j`/`k` step through them, stopping at either end. With none selected,
 * `j` starts at the first and `k` at the last. */
export function stepExecution(view: Pick<RunView, 'executions'>, current: number | undefined, delta: number) {
  const list = view.executions;
  if (!list.length) return undefined;
  const last = list.length - 1;
  const index = current == null ? -1 : list.findIndex((e) => e.id === current);
  if (index < 0) return list[delta > 0 ? 0 : last];
  return list[clamp(index + delta, 0, last)];
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}
