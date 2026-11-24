/** A slice as a card (a node of the slice graph), and the same cards as a list for narrow screens. */
import type { ReactNode } from 'react';
import { AlertTriangle, Check, Flag, LoaderCircle, SkipForward, XCircle } from 'lucide-react';
import type { SliceView } from '../types';
import { APPROVAL_SLICE, APPROVAL_SLICE_HINT, sliceCardLabel, sliceCardMeta, sliceCardState, sliceTone, type CardIcon, type StatusOf } from '../features-model';

export type SliceCardData = { slice: SliceView; selected: boolean; now: number; daemonUp: boolean; pollSecs?: number; statusOf: StatusOf };

/** The picture for each state icon (the words are in `sliceCardState`). */
const CARD_ICON: Record<NonNullable<CardIcon>, ReactNode> = {
  done: <Check size={13} aria-hidden />,
  approval: <Flag size={13} aria-hidden />,
  failed: <XCircle size={13} aria-hidden />,
  interrupted: <AlertTriangle size={13} aria-hidden />,
  skipped: <SkipForward size={13} aria-hidden />,
  running: <LoaderCircle size={13} className="spin" aria-hidden />,
};

/** A card as a screen reader hears it (its state line in words). */
export function cardLabel(slice: SliceView, now: number, daemonUp: boolean, statusOf: StatusOf, pollSecs?: number): string {
  return sliceCardLabel(slice, sliceCardState(slice, now, daemonUp, statusOf, pollSecs).text);
}

export function SliceCard({ slice, selected, now, daemonUp, pollSecs, statusOf, list = false }: SliceCardData & { list?: boolean }) {
  const tone = sliceTone(slice.status);
  const state = sliceCardState(slice, now, daemonUp, statusOf, pollSecs);
  const meta = sliceCardMeta(slice, now);
  return (
    <div className={`flow-card slice-card state-${slice.status} tone-card-${tone} ${selected ? 'selected' : ''}`}>
      <div className="card-head">
        <span className="slice-id tabular">{slice.id}</span>
        <span className="card-name" title={slice.title}>
          {slice.title}
        </span>
        {slice.checkpoint && (
          <span className="slice-checkpoint" title={slice.approved ? `${APPROVAL_SLICE}, approved` : APPROVAL_SLICE_HINT}>
            <Flag size={12} role="img" aria-label={APPROVAL_SLICE} />
          </span>
        )}
      </div>
      <div className="card-meta slice-meta">
        <span className={`card-state tone-text-${tone}${state.wraps ? ' state-wraps' : ''}`}>
          {state.icon && CARD_ICON[state.icon]}
          <span>{state.text}</span>
          {state.timer && <span className="tabular"> {state.timer}</span>}
        </span>
        {list && slice.deps.length > 0 && <span>after {slice.deps.join(', ')}</span>}
        {meta.length > 0 && <span className="tabular muted">{meta.join(' · ')}</span>}
      </div>
    </div>
  );
}

type SliceListProps = { slices: SliceView[]; selected?: string; now: number; daemonUp: boolean; pollSecs?: number; onOpen: (id: string) => void };

/** The same cards in plan order, each naming what it comes after (narrow screens and narrow panes). */
export function SliceList({ slices, selected, now, daemonUp, pollSecs, onOpen }: SliceListProps) {
  const statusOf = (id: string) => slices.find((s) => s.id === id)?.status;
  return (
    <ol className="slice-list" aria-label="Slices">
      {slices.map((s) => (
        <li key={s.id}>
          <button
            type="button"
            className="slice-list-item"
            data-id={s.id}
            onClick={() => onOpen(s.id)}
            aria-current={s.id === selected ? 'true' : undefined}
            aria-label={cardLabel(s, now, daemonUp, statusOf, pollSecs)}
          >
            <SliceCard slice={s} selected={s.id === selected} now={now} daemonUp={daemonUp} pollSecs={pollSecs} statusOf={statusOf} list />
          </button>
        </li>
      ))}
    </ol>
  );
}
