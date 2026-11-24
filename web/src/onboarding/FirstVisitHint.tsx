// Home's one-line hint for a first visit, until it is dismissed (remembered in localStorage).
import { X } from 'lucide-react';
import { useStoredState } from '../hooks';

export const HINT_KEY = 'zerochat.firstVisitHintDismissed';

export function FirstVisitHintView({ onDismiss }: { onDismiss: () => void }) {
  return (
    <p className="first-visit-hint" role="note">
      <span>
        A run is one <code>zeroshot run</code>; click a node to read its chat or message it.
      </span>
      <button type="button" className="icon-button tiny" aria-label="Dismiss hint" title="Dismiss" onClick={onDismiss}>
        <X size={14} aria-hidden />
      </button>
    </p>
  );
}

export function FirstVisitHint() {
  // useStoredState reads and writes through safeStorage, which catches blocked storage.
  const [dismissed, setDismissed] = useStoredState(HINT_KEY, false);
  return dismissed ? null : <FirstVisitHintView onDismiss={() => setDismissed(true)} />;
}
