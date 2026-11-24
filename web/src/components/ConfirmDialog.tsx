import { useState, type ReactNode } from 'react';
import { LoaderCircle } from 'lucide-react';
import { errorMessage } from '../api';
import { Modal } from './Modal';
import { ErrorBox } from './feedback';

/** A yes/no modal before an action that writes (force-stop, resume, skip a slice …): Cancel has the
 * focus, the confirm button shows progress, and a refusal is shown in the dialog. */
export function ConfirmDialog({
  title,
  children,
  confirmLabel,
  danger,
  onConfirm,
  onClose,
}: {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  onConfirm: () => Promise<void>;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <Modal
      title={title}
      onClose={onClose}
      footer={
        <>
          {error && <ErrorBox compact message={error} />}
          <div className="modal-buttons">
            {/* The dialog's default focus target (not the icon-only Close) is the safe choice. */}
            <button type="button" className="button" autoFocus onClick={onClose}>
              Cancel
            </button>
            <button
              type="button"
              className={`button ${danger ? 'danger solid' : 'primary'}`}
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                setError(null);
                try {
                  await onConfirm();
                } catch (cause) {
                  setError(errorMessage(cause));
                  setBusy(false);
                }
              }}
            >
              {busy && <LoaderCircle size={14} className="spin" aria-hidden />} {confirmLabel}
            </button>
          </div>
        </>
      }
    >
      {children}
    </Modal>
  );
}
