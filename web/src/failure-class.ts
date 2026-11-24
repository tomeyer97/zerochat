import type { Failure, RunView } from './types';
import type { Tone } from './run-status';
import { timeOfDay, untilText } from './format';

/** Why a run failed, in the server's words (`summary.failureClass`): a label and what it means,
 * and a tone for the classes that are not the run's own fault. */
export const failureClassInfo: Record<string, { label: string; explain: string; tone?: Tone }> = {
  work: { label: 'Work', explain: 'The agents did the work, but a reviewer did not accept it.' },
  agent: { label: 'Agent', explain: 'An agent failed, not the code or its provider.' },
  provider: {
    label: 'Provider',
    explain: "The model provider's side: not answering, a broken stream, an outage, a usage or rate limit. Usually temporary.",
    tone: 'warn',
  },
  setup: {
    label: 'Setup',
    explain: 'The run is set up wrong (credentials, model, tools); retrying unchanged fails the same way.',
  },
  orchestration: { label: 'Orchestration', explain: 'Zeroshot itself failed, not the code or the agents.' },
  delivery: { label: 'Delivery', explain: 'The work was done, but delivering it (push, pull request) failed.' },
  stopped: { label: 'Stopped', explain: 'The run was force-stopped.' },
};

export function failureClass(value: string) {
  return failureClassInfo[value] ?? { label: value || 'Failure', explain: '' };
}

// The failure reasons the run page treats specially, named here beside the classes above.

/** Force-stopped on purpose: not a failure to explain, one line and the actions. */
export function isStopped(failure: Pick<Failure, 'class'>): boolean {
  return failure.class === 'stopped';
}

/** The controller went away mid-run: Zeroshot's `controller_not_running` reason, or the cause
 * the server writes for it (`controller lost`). */
export function isStalled(failure: Pick<Failure, 'reason' | 'cause'>): boolean {
  return failure.reason === 'controller_not_running' || failure.cause === 'controller lost';
}

// What the failure card says beside its actions.

/** The fix leads: retrying unchanged would fail the same way, so the server picked no action
 * (unknown model, no access, a node timeout, a reviewer that rejects every round). */
export function fixLeads(failure: Pick<Failure, 'primary' | 'fix'>): boolean {
  return failure.primary == null && !!failure.fix;
}

/**
 * Starting again (or resuming) with the same settings cannot help: the server picked no action,
 * the fix is a change to the runtime file, or it is a setup failure caused by the model or the
 * config. Only a replay that changes settings can get past it.
 */
export function unchangedFails(failure: Pick<Failure, 'primary' | 'fix' | 'class' | 'cause'>): boolean {
  if (failure.class === 'stopped') return false;
  if (failure.primary == null) return true;
  if (failure.fix && /runtime file/i.test(failure.fix)) return true;
  return failure.class === 'setup' && /model|config|runtime/i.test(failure.cause);
}

/** The start-again dialog's one-line warning when `unchangedFails`, with the fix to make first. */
export function unchangedWarning(failure: Pick<Failure, 'primary' | 'fix' | 'class' | 'cause'>): string | null {
  if (!unchangedFails(failure)) return null;
  return failure.fix ? `Unchanged, this will fail the same way. Fix first: ${failure.fix}` : `Unchanged, this will likely fail the same way (${failure.cause}).`;
}

/** Zeroshot is still closing the failed run (its postmortem): it takes no action until then. */
export function isClosing(view: Pick<RunView, 'phase' | 'status'>): boolean {
  return view.phase === 'running' && view.status === 'failed';
}

/** The note beside a failure's actions: Zeroshot is still closing the run, or the run has no
 * checkpoint to replay from (only when its checkpoint list is empty; a stalled run has some). */
export function actionsNote(view: Pick<RunView, 'phase' | 'status' | 'local' | 'resumedAs'>): string | null {
  if (isClosing(view)) return 'Zeroshot is finishing the run: actions in a moment';
  if (view.local && !view.local.hasCheckpoints && !view.resumedAs) return 'no checkpoints';
  return null;
}

/** When a usage limit resets, beside the Resume button: `limit resets in 12 min` before,
 * `limit reset at 1:24 PM` after. */
export function resetNote(failure: Pick<Failure, 'resetsAt'>, now: number): string | null {
  const at = failure.resetsAt;
  if (at == null) return null;
  return at > now ? `limit resets ${untilText(at, now)}` : `limit reset at ${timeOfDay(at, now)}`;
}
