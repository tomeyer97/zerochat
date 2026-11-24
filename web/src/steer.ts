/**
 * Messages to a running agent ("steers"): what the chat shows next to one (its status chip) and
 * what the composer says about it. Zeroshot 16.1 records a steer with a trailing `\n\n[status]`
 * tag: `received`, `sent` (not received yet) or `not received`, plus `, to all agents` for a
 * run-wide message. Runs recorded by the older patched Zeroshot use `delivered` (no tag),
 * `queued` and `rejected`; they are still shown.
 */
import type { RunView, SteerResult, TranscriptItem } from './types';

/** A message sent from this page that the transcript does not show yet. */
export type Sent = { text: string; at: number; status: string; reason?: string | null };

export type SteerTone = 'ok' | 'warn' | 'bad';

/** Messages sent from here that the transcript has not recorded yet (it shows them once it has). */
export function pendingSent(sent: Sent[], items: TranscriptItem[]): Sent[] {
  const recorded = items.filter((i) => i.kind === 'steer').map((i) => steerBody(i.text ?? ''));
  return sent.filter((s) => !recorded.includes(s.text.trim()));
}

/** A recorded steer item's message, without its trailing `\n\n[status]` tag. */
export function steerBody(text: string): string {
  return text.split('\n\n[')[0].trim();
}

/** A recorded steer item's status tag (`null`: none, the older Zeroshot's "delivered"). */
export function steerTag(text: string): string | null {
  const m = /\n\n\[([^\]]+)\]$/.exec(text);
  return m ? m[1] : null;
}

/**
 * Every status a message to an agent can have, in one place: `chip` is the word or two next to
 * the message in the chat (the same while it is in flight and once the transcript records it),
 * `line` what the composer says under the box (`null`: nothing, the message shows in the chat).
 * Zeroshot 16.1 statuses come first; `delivered`/`queued`/`rejected` are the older patched
 * Zeroshot's.
 */
export function steerStatus(status: string | null, reason?: string | null, harness?: string | null): { tone: SteerTone; chip: string; line: string | null } {
  const [state, scope] = (status ?? 'delivered').split(', ');
  const all = scope === 'to all agents' ? ' · sent to all agents' : '';
  switch (state) {
    case 'received':
      return { tone: 'ok', chip: `received${all}`, line: null };
    case 'sent':
      return { tone: 'ok', chip: `sent, not received yet${all}`, line: null };
    case 'not received':
      return { tone: 'bad', chip: `not received${all}`, line: null };
    case 'delivered':
      return { tone: 'ok', chip: 'sent', line: null };
    // Copilot has no live input: a queued message reaches the running turn with the agent's next
    // tool call (its preToolUse hook); other harnesses take it into their next prompt.
    case 'queued':
      return { tone: 'ok', chip: harness === 'copilot' ? 'queued for its next tool call' : 'queued for its next turn', line: null };
    case 'rejected':
      return { tone: 'bad', chip: 'not delivered: rejected', line: `Rejected${reason ? `: ${reason}` : ''}.` };
    default:
      return { tone: 'warn', chip: `not delivered: ${state.replace(/_/g, ' ')}`, line: `${state}${reason ? `: ${reason}` : ''}` };
  }
}

/** The chip of a recorded or in-flight message (see `steerStatus`). */
export function steerTagLabel(tag: string | null, harness?: string | null): { text: string; bad?: boolean } {
  const status = steerStatus(tag, null, harness);
  return status.tone === 'ok' ? { text: status.chip } : { text: status.chip, bad: true };
}

/** What to say under the composer: nothing when it went through (the message shows in the chat). */
export function steerStatusText(result: SteerResult): { tone: SteerTone; text: string } | null {
  const status = steerStatus(result.status, result.reason);
  return status.line == null ? null : { tone: status.tone, text: status.line };
}

/** Who a message from this agent's chat reaches: with a Zeroshot that can steer one node
 * (`steer_node`), this agent; otherwise every agent of the run (Zeroshot 16.1 steers runs). */
export function steerScope(view: Pick<RunView, 'capabilities'>): 'node' | 'run' {
  return (view.capabilities ?? []).includes('steer_node') ? 'node' : 'run';
}

/** What the composer says before sending, so nobody is surprised who reads it. */
export function steerScopeNote(scope: 'node' | 'run', node: string): string {
  return scope === 'node' ? `To this agent (${node}) only` : 'To all agents in this run, not only this one';
}
