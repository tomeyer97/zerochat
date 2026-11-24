import type { RunListing, RunView } from './types';

/** The colour family a status is drawn in (`tone-*` classes). */
export type Tone = 'running' | 'ok' | 'bad' | 'warn' | 'neutral' | 'muted';

/** Every status a run can be in, as the server names it; `unknown` is ours, for anything else. */
const RUN_STATUSES = ['running', 'succeeded', 'failed', 'stalled', 'stopped', 'stopping', 'admitted', 'suspended'] as const;

export type RunStatus = (typeof RUN_STATUSES)[number] | 'unknown';

function asRunStatus(value: string | undefined | null): RunStatus | null {
  return RUN_STATUSES.find((status) => status === value) ?? null;
}

/** The status of a run listing: the server's summary when it has one, else the terminal status or live phase. */
export function listingStatus(run: RunListing): RunStatus {
  const summary = asRunStatus(run.summary?.status);
  if (summary) return summary;
  if (run.terminal?.reason === 'force_stopped') return 'stopped';
  const terminal = run.terminal?.status;
  if (terminal === 'succeeded' || terminal === 'failed') return terminal;
  if (terminal) return 'failed';
  const phase = asRunStatus(run.phase);
  if (phase) return phase;
  if (run.phase === 'finished') return 'succeeded';
  return 'unknown';
}

export function runStatus(view: Pick<RunView, 'status'>): RunStatus {
  return asRunStatus(view.status) ?? 'unknown';
}

/** Red is for failures only; running is blue, stalled amber, stopped grey. */
export const runStatusInfo: Record<RunStatus, { label: string; tone: Tone; explain: string }> = {
  running: { label: 'Running', tone: 'running', explain: 'Agents are working on this run right now.' },
  succeeded: { label: 'Succeeded', tone: 'ok', explain: 'The graph reached a succeed exit.' },
  failed: { label: 'Failed', tone: 'bad', explain: 'The graph ended without succeeding.' },
  stalled: { label: 'Stalled', tone: 'warn', explain: 'The run says it is running, but its controller process is gone.' },
  stopped: { label: 'Stopped', tone: 'neutral', explain: 'The run was force-stopped.' },
  stopping: { label: 'Stopping', tone: 'neutral', explain: 'A stop was requested; Zeroshot is winding the run down.' },
  admitted: { label: 'Starting', tone: 'running', explain: 'Zeroshot accepted the run and is preparing it.' },
  suspended: {
    label: 'Suspended',
    tone: 'neutral',
    explain: 'Paused at a checkpoint: it continues when it is resumed or its question is answered.',
  },
  unknown: { label: 'Unknown', tone: 'muted', explain: 'zerochat could not tell what state this run is in.' },
};

export function isLive(status: RunStatus) {
  return status === 'running' || status === 'stopping' || status === 'admitted';
}

/** A run page still needs live updates: actively running, or stalled (its controller is gone,
 * but Zeroshot hasn't recorded that as a terminal outcome yet, so it can still close any
 * moment). A settled terminal run (succeeded/failed/stopped) never changes again. */
export function isWatchable(status: RunStatus) {
  return isLive(status) || status === 'stalled';
}
