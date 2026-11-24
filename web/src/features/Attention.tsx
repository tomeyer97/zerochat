import { api } from '../api';
import type { FeatureDetail } from '../types';
import { attentionItems, type AttentionItem } from '../features-model';
import { timeOfDay } from '../format';
import { ErrorBox } from '../common';
import { ActionButton } from './ActionButton';
import type { OnDone } from './FeatureHeader';
import { useBusyAction } from './page-hooks';

/** What needs a person, each with the button that does it. */
export function Attention({ detail, now, onOpen, onDone }: { detail: FeatureDetail; now: number; onOpen: (slice?: string) => void; onDone: OnDone }) {
  const { busy: approving, error, run } = useBusyAction<string>();
  const approve = (id: string) =>
    run(
      id,
      async () => onDone((await api.sliceAction(detail.name, id, { action: 'approve' })).message),
      // Reload either way: a refused approval usually means the state moved on.
      () => onDone('', 'info')
    );
  const items = attentionItems(detail);
  if (!detail.issue && !items.length) return null;
  return (
    <>
      <ul className="attention-list">
        {detail.issue && <ErrorBox key="issue" compact title="Can't read this feature; nothing can run until it is fixed." message={detail.issue} />}
        {items.map((item) => (
          <AttentionLine key={itemKey(item)} item={item} now={now} onOpen={onOpen} approving={approving} onApprove={(id) => void approve(id)} />
        ))}
      </ul>
      {error && <ErrorBox compact message={error} />}
    </>
  );
}

function itemKey(item: AttentionItem): string {
  switch (item.kind) {
    case 'approve':
      return `a${item.slice}`;
    case 'failed':
      return `f${item.slice}`;
    case 'stopped':
      return 'start';
    default:
      return item.kind;
  }
}

function AttentionLine({
  item,
  now,
  onOpen,
  approving,
  onApprove,
}: {
  item: AttentionItem;
  now: number;
  onOpen: (slice?: string) => void;
  approving: string | null;
  onApprove: (slice: string) => void;
}) {
  switch (item.kind) {
    case 'paused':
      return (
        <li className="attention-item tone-warn">
          <span title={item.reason}>
            {item.text}
            {item.slice && (
              <>
                {' '}
                (last:{' '}
                <button type="button" className="text-button inline-link" onClick={() => onOpen(item.slice!)}>
                  {item.slice}
                </button>
                )
              </>
            )}
            {item.since && <span className="muted"> · since {timeOfDay(item.since, now)}</span>}. Nothing new starts until you unpause.
          </span>
        </li>
      );
    case 'approve':
      return (
        <li className="attention-item tone-warn">
          <span>
            <strong>{item.slice}</strong> {item.title} {item.waits}
            {item.since ? <span className="muted"> since {timeOfDay(item.since, now)}</span> : ''}
            {item.after}.
          </span>
          <span className="attention-buttons">
            {/* A slice waiting for review is approved in its panel, next to its diff. */}
            <button type="button" className={item.review ? 'button small primary' : 'button small'} onClick={() => onOpen(item.slice)}>
              Review
            </button>
            {!item.review && (
              <ActionButton name={item.slice} busy={approving} primary onClick={() => onApprove(item.slice)}>
                Approve {item.slice}
              </ActionButton>
            )}
          </span>
        </li>
      );
    case 'failed':
      return (
        <li className="attention-item tone-bad">
          <span>
            <strong>{item.slice}</strong> {item.title}: {item.failure}
            {item.after}.
          </span>
          <span className="attention-buttons">
            <button type="button" className="button small" onClick={() => onOpen(item.slice)}>
              Open {item.slice}
            </button>
          </span>
        </li>
      );
    case 'note':
      return (
        <li className="attention-item tone-warn">
          <span>{item.text}</span>
        </li>
      );
    case 'stopped':
      return (
        <li className={`attention-item tone-${item.tone}`}>
          <span>{item.text}</span>
        </li>
      );
  }
}
