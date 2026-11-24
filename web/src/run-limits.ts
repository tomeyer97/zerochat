/**
 * What a run's source cannot show or do: data its Zeroshot does not record, actions its Zeroshot
 * lacks, and sources zerochat only observes.
 *
 * Two source names appear below. `pb` (ProgramBench runner) is a separate benchmarking tool that
 * runs Zeroshot graphs in a sandbox; its runs show up under the Benchmarks page and as a source
 * whose context kind is `benchmark`. `zsf` (Zeroshot features) is the slice/feature scheduler
 * behind the Features page; its state directories are another configured source.
 */
import type { RunView } from './types';

/** What this Zeroshot does not record, in a few words each; empty when nothing is missing. */
export function missingData(view: Pick<RunView, 'capabilities' | 'features' | 'executions' | 'targetKind'>): string[] {
  return missingDataWhy(view).map((m) => m.what);
}

/** `missingData`, each with why it is missing (the "limited data" popover shows both). */
export function missingDataWhy(view: Pick<RunView, 'capabilities' | 'features' | 'executions' | 'targetKind'>): { what: string; why: string }[] {
  const caps = view.capabilities ?? [];
  const out: { what: string; why: string }[] = [];
  if (view.features && !view.features.exactTimes && view.executions.length > 0) {
    out.push({
      what: 'exact node times (times marked ≈ are inferred from log lines)',
      why: 'this Zeroshot version does not record them',
    });
  }
  if (view.features && !view.features.toolCalls && view.targetKind !== 'local') {
    out.push({ what: 'tool calls', why: "this Zeroshot version does not record them, and the agent's own log is not on this machine" });
  }
  if (view.features && !view.features.workspaceSnapshots) {
    out.push({ what: 'file changes per node', why: 'this Zeroshot version does not record them' });
  }
  // An observe-only source offers no actions at all, whatever its Zeroshot supports (see
  // `observeOnly`); that is not something this Zeroshot fails to record.
  if (view.targetKind === 'observe') return out;
  if (!caps.includes('steer')) out.push({ what: 'messages to running agents', why: 'they need Zeroshot 16.1 or later (zeroshot steer)' });
  return out;
}

/** Why an action this Zeroshot cannot do is missing, in public words (one wording for the
 * composer, the replay dialog and anything else that names it). */
export function capabilityHint(capability: 'steer' | 'replay'): string {
  switch (capability) {
    case 'steer':
      return 'Sending messages to running agents needs Zeroshot 16.1 or later (`zeroshot steer`).';
    case 'replay':
      return 'Zeroshot resumes a checkpoint unchanged; to change prompts, models or the graph, start a new run.';
  }
}

// ---------- observe-only sources ----------

/** Configured sources (pb, `[[source]]` directories from zerochat's config, zsf's state
 * directories) are observed, never driven: the server refuses Stop, Resume, Replay and Steer for
 * them (`require_actions`), and for pb the controller runs inside its sandbox, out of zerochat's
 * reach. The run page offers none of those actions for such a run, with this note instead. */
export function observeOnly(view: Pick<RunView, 'targetKind'>): boolean {
  return view.targetKind === 'observe';
}

export function observeOnlyNote(view: Pick<RunView, 'targetName' | 'context'>): string {
  const benchmark = view.context?.kind === 'benchmark';
  const who = benchmark ? 'pb' : view.targetName || 'this source';
  // A label group (`label:…`) collects runs from several batches; there is no one batch to stop.
  const inRealBatch = benchmark && !!view.context?.parent && !view.context.parent.startsWith('label:');
  return (
    `Observed only: ${who} runs this, so zerochat does not stop, resume, replay or message it ` +
    '(that would disturb the sandbox and accounting that produced it).' +
    (inRealBatch ? ' To stop it, stop its batch.' : '')
  );
}
