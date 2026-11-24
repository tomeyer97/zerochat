// What a replay batch replayed, in words.
import type { ReplayContext } from '../types';
import { bareModel } from './harness-text';
import { scoreText } from './scores';

/** `build#1`, or `triage#1 → repair#1` for a segment. */
export function replaySegmentText(r: Pick<ReplayContext, 'node' | 'through'>): string {
  return r.through && r.through !== r.node ? `${r.node} → ${r.through}` : r.node;
}

/** `on sonnet-5 (recorded: haiku-4-5)`, or just the model when it did not change. */
export function replayModelText(r: Pick<ReplayContext, 'models' | 'recordedModel'>): string | null {
  if (r.models.length === 0) return r.recordedModel ? `on ${bareModel(r.recordedModel)} (as recorded)` : null;
  const now = r.models.map(bareModel).join(', ');
  if (r.recordedModel && r.models.length === 1 && r.models[0] === r.recordedModel) return `on ${now} (as recorded)`;
  return `on ${now}${r.recordedModel ? ` (recorded: ${bareModel(r.recordedModel)})` : ''}`;
}

/** The pieces of one replay's sentence, which the batch page links and styles:
 * "Replay of clone-v1 · gron (its batch) from build#1 on sonnet-5 (recorded: haiku-4-5)
 * · started from the pristine task workspace · sweep s1 · recorded score after build#1: 0.53
 * (run final 0.62)". Pieces that do not apply are `null`. */
export type ReplaySentence = {
  /** The recorded run, by label and task (its pb run id when it has neither). */
  source: string;
  segment: string;
  model: string | null;
  base: string | null;
  sweep: string | null;
  /** The last node replayed: the recorded score is the one right after it. */
  end: string;
  /** `(run final 0.62)`, when the recorded run's final score differs from the one after `end`. */
  runFinal: string | null;
};

export function replaySentence(r: ReplayContext): ReplaySentence {
  const runFinal = r.sourceScore != null && r.sourceScore !== r.recordedScore ? `(run final ${scoreText(r.sourceScore)})` : null;
  return {
    source: [r.sourceLabel, r.sourceTask].filter(Boolean).join(' · ') || r.sourceRun,
    segment: replaySegmentText(r),
    model: replayModelText(r),
    base: r.base,
    sweep: r.sweepId ? r.sweepLabel || r.sweepId : null,
    end: r.through ?? r.node,
    runFinal,
  };
}
