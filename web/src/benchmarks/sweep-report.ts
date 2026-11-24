// The sweep report (`pb sweep-report`): what each table row shows, the summary under a mapping,
// and when writing the suggested runtime is offered. pb does the math; this only words it.
import type { SweepModelStat, SweepRecommendation, SweepSuggestion } from '../types';
import { usd } from '../format';
import { scoreText } from './scores';
import { signedPp, signedUsd } from './batch-compare';
import { resourceKey } from './resource-key';

/** Where "Write runtime…" proposes to write (relative to pb_root), next to pb's own runtime.json. */
export const DEFAULT_SUGGESTED_RUNTIME_PATH = 'harness/runtime.suggested.json';

export type SweepReportFilter = { ids: string[]; label?: string; task?: string; budget?: string };

export function sweepReportKey(route: SweepReportFilter): string {
  return resourceKey('benchmarks/sweep-report', route.ids.join(','), route.label, route.task, route.budget);
}

/** One model's row in a node's table, as text and classes. The baseline row leaves its Δ columns
 * blank (it is what the others are measured against); a model with no scored replay shows how
 * its replays ended instead of a score. */
export type SweepRowCells = {
  n: string;
  score: string;
  delta: string;
  noiseClass: string;
  dnode: string;
  dcost: string;
  perUsd: string;
};

export function sweepRowCells(m: SweepModelStat): SweepRowCells {
  const vsBaseline = (text: string) => (m.isBaseline ? '' : text);
  const endings = Object.entries(m.verdicts)
    .map(([verdict, count]) => `${verdict} ×${count}`)
    .join(', ');
  return {
    n: m.replays !== m.n ? `${m.n}/${m.replays}` : `${m.n}`,
    score: m.mean != null ? scoreText(m.mean) : endings || '—',
    delta: vsBaseline(signedPp(m.delta)),
    noiseClass: m.noise === 'beyond noise' ? 'bm-signal-pos' : 'muted',
    dnode: vsBaseline(signedUsd(m.dnode)),
    dcost: vsBaseline(signedUsd(m.dcost)),
    perUsd: m.perUsd != null ? `${(m.perUsd * 100).toFixed(1)}pp/$` : '',
  };
}

/** The recommendation's colour: green to upgrade, muted when pb cannot tell, plain otherwise. */
export function recommendationTone(action: SweepRecommendation['action']): string {
  if (action === 'upgrade') return 'bm-signal-pos';
  if (action === 'unknown') return 'muted';
  return '';
}

/** The sentence under a mapping table: how many nodes keep their model, and the expected change
 * against the recorded runtime. */
export function mappingSummary(s: SweepSuggestion): string {
  const unchanged = Object.keys(s.mapping).length - s.changes.length;
  const cost = s.baseCost != null ? ` (cost: ${usd(s.baseCost)} → ${usd(s.estCost)})` : '';
  return `${unchanged} other node(s) unchanged. Expected vs the recorded runtime: Δscore ${signedPp(s.deltaScore)}, Δcost ${signedUsd(s.deltaCost)} per run${cost}`;
}

/** Why "Write runtime…" is off, or `null` when it is offered: a mapping that changes nothing
 * would write the recorded runtime again, and one over budget does not do what was asked. */
export function writeDisabledReason(toWrite: Pick<SweepSuggestion, 'changes' | 'overBudget'>): string | null {
  if (toWrite.changes.length === 0) return 'Nothing would change.';
  if (toWrite.overBudget) return "This mapping doesn't fit the budget.";
  return null;
}
