/**
 * Badges for the two status vocabularies. Labels, tones and explanations live in ../status.ts (pure data,
 * covered by the status goldens); the icons are React components, so they stay here.
 *   a run's `RunStatus`   -> `StatusDot` (run list, app header) and `StatusPill` (run header)
 *   a node's `NodeState`  -> `StateIcon` (graph cards) and `StatePill` (node panel)
 * Both pills share one shape (`Pill`): tone colour, explanation on hover.
 */
import type { ReactNode } from 'react';
import { CircleStop, CheckCircle2, Circle, CircleDashed, LoaderCircle, MinusCircle, OctagonX, XCircle } from 'lucide-react';
import type { NodeState, RunStatus, Tone } from '../status';
import { execStateInfo, runStatusInfo } from '../status';

/** The pill both vocabularies share: tone colour, explanation on hover. */
function Pill({ info, small = false, children }: { info: { tone: Tone; explain: string }; small?: boolean; children: ReactNode }) {
  return (
    <span className={`pill ${small ? 'small ' : ''}tone-${info.tone}`} title={info.explain}>
      {children}
    </span>
  );
}

export function StatusDot({ status, title }: { status: RunStatus; title?: string }) {
  const info = runStatusInfo[status];
  const label = title ?? info.label;
  return <span className={`status-dot tone-${info.tone} status-${status}`} title={label} aria-label={label} role="img" />;
}

export function StatusPill({ status }: { status: RunStatus }) {
  const info = runStatusInfo[status];
  const busy = status === 'running' || status === 'admitted';
  return (
    <Pill info={info}>
      {busy ? <LoaderCircle size={13} className="spin" aria-hidden /> : null}
      {info.label}
    </Pill>
  );
}

const stateIcons: Record<NodeState, typeof Circle> = {
  idle: Circle,
  running: LoaderCircle,
  succeeded: CheckCircle2,
  accepted: CheckCircle2,
  rejected: XCircle,
  failed: OctagonX,
  interrupted: CircleStop,
  voided: MinusCircle,
};

export function StateIcon({ state, size = 15 }: { state: NodeState; size?: number }) {
  const Icon = stateIcons[state] ?? CircleDashed;
  return <Icon size={size} className={`state-icon state-${state} ${state === 'running' ? 'spin' : ''}`} aria-hidden />;
}

export function StatePill({ state, compact = false }: { state: NodeState; compact?: boolean }) {
  const info = execStateInfo[state];
  return (
    <Pill info={info} small>
      <StateIcon state={state} size={12} />
      {!compact && info.label}
    </Pill>
  );
}
