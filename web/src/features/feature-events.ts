/**
 * The feature timeline's events in words. Each event is one line of zsf's `events.jsonl`: a
 * `kind` (zsf's `EventKind`, crates/zsf/src/model.rs) and a free-text `detail` that zsf's engine
 * writes (crates/zsf/src/engine.rs, at each `emit`). The details are English, not structured, so
 * the parsers below match the exact sentences zsf writes; each says which one. Kinds this table
 * does not know (a newer zsf) read as their name with spaces.
 */
import type { FeatureEvent } from '../types';
import type { Tone } from '../status';
import { verdictLine } from './hook-output';
import { ATTEMPT_KIND_WORDS, RESULT_WORDS, REVIEW_WAIT, reasonSummary } from './slice-words';
import { pausedSummary } from './feature-words';

type Event = Pick<FeatureEvent, 'kind' | 'detail' | 'slice'>;

type EventWords = {
  /** The headline: what happened. */
  what: string;
  /** Red for a failure, amber for what needs a person; muted otherwise. */
  tone?: Tone;
  /** Only repeats a neighbour: hidden until "Show all events". */
  hidden?: boolean;
  /** The detail as shown, from the trimmed detail (default: none). */
  detail?: (d: string, e: Event) => string;
  /** The event's run is worth opening from the timeline (`run ›`). */
  linksRun?: boolean;
};

const asIs = (d: string) => d;
/** The short commit of a `merged` / `accepted` detail (`<sha> …`). */
const shortCommit = (d: string) => d.split(' ')[0].slice(0, 7);

/** Every kind zsf writes: its `EventKind` (crates/zsf/src/model.rs) in snake_case. */
export type FeatureEventKind =
  | 'initialized'
  | 'daemon_started'
  | 'daemon_stopped'
  | 'paused'
  | 'waiting'
  | 'done'
  | 'launched'
  | 'launch_failed'
  | 'run_finished'
  | 'quality_failed'
  | 'run_stopped'
  | 'infra_failure'
  | 'interrupted'
  | 'resumed'
  | 'resume_failed'
  | 'gate_failed'
  | 'merged'
  | 'conflict'
  | 'integration_failed'
  | 'checkpoint_waiting'
  | 'hook_failed'
  | 'started'
  | 'stopped'
  | 'unpaused'
  | 'approved'
  | 'retried'
  | 'accepted'
  | 'skipped'
  | 'edited'
  | 'request_rejected';

/** Each kind's words, in `EventKind`'s order. */
const EVENTS: Record<FeatureEventKind, EventWords> = {
  // The feature and its daemon.
  initialized: { what: 'created' },
  daemon_started: { what: 'daemon started', hidden: true },
  daemon_stopped: { what: 'daemon stopped', hidden: true },
  // `{n} infrastructure failures in a row (last: …)`.
  paused: { what: 'paused', tone: 'warn', detail: (d) => pausedSummary(d).text.replace(/^Paused /, '') },
  waiting: { what: 'held', detail: asIs },
  // `{merged} merged into {branch}[, {n} skipped]`; older zsf wrote `every slice is merged into {branch} or skipped`.
  done: { what: 'done', detail: (d) => d.replace(/^every slice is merged into (\S+) or skipped$/, 'merged into $1') },

  // A slice's run.
  // `{attempt kind} attempt[…]`.
  launched: {
    what: 'started',
    linksRun: true,
    detail: (d) => {
      const m = /^(\S+) attempt(.*)$/.exec(d);
      return m ? `${ATTEMPT_KIND_WORDS[m[1]] ?? m[1]}${m[2]}` : d;
    },
  },
  launch_failed: { what: RESULT_WORDS.launch_failed, tone: 'bad', detail: (d) => reasonSummary(`launch failed: ${d}`).headline.replace(/^Launch failed: /, '') },
  // The run's terminal status, `succeeded` or `failed: <reason>`.
  run_finished: { what: 'run ended', detail: (d) => d.replace(/_/g, ' ') },
  quality_failed: { what: RESULT_WORDS.quality_failed, tone: 'bad' },
  run_stopped: { what: 'run stopped' },
  // `#<attempt>: <reason>`.
  infra_failure: { what: 'infrastructure failure', tone: 'bad', detail: (d) => reasonSummary(d.replace(/^#\d+: /, '')).headline },
  interrupted: { what: 'run lost', tone: 'warn', linksRun: true, detail: (d) => (/runtime_lost/.test(d) ? 'its controller was lost' : d) },
  resumed: { what: 'resumed in a new run', linksRun: true },
  resume_failed: { what: 'resume failed', tone: 'bad', detail: asIs },

  // A slice's integration onto the feature branch.
  gate_failed: { what: RESULT_WORDS.gate_failed, tone: 'bad', linksRun: true, detail: verdictLine },
  merged: { what: RESULT_WORDS.merged, detail: shortCommit },
  // `{why}; running it once more from the new tip` when zsf re-runs it.
  conflict: { what: RESULT_WORDS.conflict, tone: 'warn', linksRun: true, detail: (d) => d.replace(/; running it once more from the new tip$/, '; re-running from the new tip') },
  integration_failed: { what: RESULT_WORDS.integration_failed, tone: 'bad', detail: asIs },
  // Waiting unmerged for review (`waiting for review, not merged (…)`); an older zsf merged first and
  // wrote `review <short sha>`, older still with `, then: zsf approve <feature> <slice>`.
  checkpoint_waiting: {
    what: REVIEW_WAIT,
    tone: 'warn',
    detail: (d) =>
      /^review [0-9a-f]/.test(d)
        ? d
            .replace(/^review /, '')
            .replace(/, then: zsf approve .*$/, '')
            .slice(0, 7)
        : '',
  },
  hook_failed: { what: 'hook failed', tone: 'bad', detail: verdictLine },

  // Requests (CLI or zerochat).
  started: { what: 'started' },
  stopped: { what: 'stopped launching' },
  unpaused: { what: 'unpaused' },
  approved: { what: 'approved' },
  // `[fresh][, with notes: …]`.
  retried: { what: 'retried by you', detail: (d) => d.replace(/^fresh/, 'from scratch') },
  accepted: { what: 'merged without the gate', detail: shortCommit },
  skipped: { what: RESULT_WORDS.skipped, detail: asIs },
  // On a slice, the changes (`spec /path/…, deps …`); on the feature, `max parallel {n}`.
  edited: {
    what: 'edited',
    detail: (d, e) => (e.slice ? d.replace(/spec \/\S+/, 'spec') : d.replace(/^max parallel /, '')),
  },
  request_rejected: { what: 'refused', detail: asIs },
};

/** The words for a kind zsf wrote; `undefined` for one this table does not know (a newer zsf). */
function wordsOf(kind: string): EventWords | undefined {
  return Object.hasOwn(EVENTS, kind) ? EVENTS[kind as FeatureEventKind] : undefined;
}

/** An event in words: `{what, detail}`; no enums, no CLI hints, no full run ids. */
export function eventText(e: Event): { what: string; detail: string } {
  const d = (e.detail ?? '').trim();
  const words = wordsOf(e.kind);
  if (!words) return { what: e.kind.replace(/_/g, ' '), detail: d };
  // A feature-level `edited` is the parallel limit changing.
  const what = e.kind === 'edited' && !e.slice ? 'parallel limit' : words.what;
  return { what, detail: words.detail ? words.detail(d, e) : '' };
}

export function eventTone(kind: string): Tone {
  return wordsOf(kind)?.tone ?? 'muted';
}

/** Whether the timeline links an event of this kind to its run. */
export function eventLinksRun(kind: string): boolean {
  return wordsOf(kind)?.linksRun ?? false;
}

/** Events that only repeat their neighbour (shown under "Show all events"). */
export function isNoiseEvent(e: Pick<FeatureEvent, 'kind' | 'detail'>): boolean {
  if (wordsOf(e.kind)?.hidden) return true;
  // "run ended: succeeded" is always followed by merged / gate failed / conflict.
  if (e.kind === 'run_finished' && e.detail === 'succeeded') return true;
  // A launch failure is logged once more as the infrastructure failure it counts as.
  if (e.kind === 'infra_failure' && /^#\d+: launch failed: /.test(e.detail ?? '')) return true;
  // zsf's "waiting for you" / "nothing can start" note (engine.rs) repeats the checkpoint or failure event before it.
  if (e.kind === 'waiting' && (!e.detail || /^(waiting for you|nothing can start)/.test(e.detail))) return true;
  return false;
}

/** Slice ids that have events, in id order (the timeline's filter chips). */
export function eventSlices(events: Pick<FeatureEvent, 'slice'>[]): string[] {
  const seen: string[] = [];
  for (const e of events) if (e.slice && !seen.includes(e.slice)) seen.push(e.slice);
  return seen.sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
}
