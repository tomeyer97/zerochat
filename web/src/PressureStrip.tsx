import type { Pressure, RunView } from './types';
import type { PressureKind } from './generated/projection';
import { fullTime, untilText } from './format';
import { nodeHref, visitOf } from './route';

/** What each kind of pressure is called when its text is not a title. */
const KIND_TITLE: Record<PressureKind, string> = {
  rate_limit: 'rate limit hit',
  retry: "retrying the provider's API",
  timeout: 'close to its timeout',
  attempts: 'last attempt',
  rounds: 'last round',
};

/** The provider's own pressures (its API is limited or retried): named with their agent, which
 * links to that agent's log. */
const PROVIDER_KINDS: PressureKind[] = ['rate_limit', 'retry'];

/** A short title, not a log line (one line, short, not one of Zeroshot's raw lines). */
function isTitle(text: string): boolean {
  return !!text.trim() && text.length <= 80 && !/[\r\n]/.test(text) && !/^Claude |toolu_/.test(text);
}

/** What the strip says for a pressure: its title, never raw log text. */
export function pressureTitle(p: Pick<Pressure, 'kind' | 'text'>): string {
  return isTitle(p.text) ? p.text : KIND_TITLE[p.kind];
}

/** One row: per node, its limits close to running out; the node is named when several run, and
 * always for the provider's pressures (the agent, linking to its log). */
export function PressureStrip({ pressures, view, now }: { pressures: Pressure[]; view: RunView; now: number }) {
  // The loop's own amber round badge on the graph already carries this one; a timeout or an
  // attempts limit has no such badge, so those still need the strip.
  const shown = pressures.filter((p) => p.level !== 'info' && p.kind !== 'rounds');
  if (!shown.length) return null;
  const groups = new Map<string, Pressure[]>();
  for (const p of shown) groups.set(p.node ?? '', [...(groups.get(p.node ?? '') ?? []), p]);
  const running = new Set(view.executions.filter((e) => e.state === 'running').map((e) => e.node));
  const nameNodes = groups.size > 1 || running.size > 1;
  return (
    <ul className="pressures" aria-label="Limits close to running out">
      {[...groups.entries()].map(([node, list]) => {
        const e = view.executions.find((x) => x.id === list[0].execution);
        const named = nameNodes || list.some((p) => PROVIDER_KINDS.includes(p.kind));
        return (
          <li key={node || '-'} className="pressure">
            {named && node && (
              <a className="pressure-node" href={nodeHref(view.target, view.runId, node, e ? visitOf(e) : {})} title={`Open ${node}'s log`}>
                {node}:
              </a>
            )}
            {list.map((p, i) => (
              <span key={`${p.kind}-${i}`} className={`pressure-item level-${p.level}`} title={p.resetsAt != null ? `Resets ${fullTime(p.resetsAt, now)}` : undefined}>
                {i > 0 && <span className="muted"> · </span>}
                {pressureTitle(p)}
                {p.resetsAt != null && ` · resets ${untilText(p.resetsAt, now)}`}
              </span>
            ))}
          </li>
        );
      })}
    </ul>
  );
}
