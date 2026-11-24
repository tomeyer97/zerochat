/** The node chat's composer: a message to the running agent (a "steer", see steer.ts). */
import { useState } from 'react';
import { Info, LoaderCircle, Send } from 'lucide-react';
import { api, ApiError, errorMessage } from './api';
import type { Execution, RunView } from './types';
import { capabilityHint, observeOnly, observeOnlyNote } from './status';
import { steerScope, steerScopeNote, steerStatusText, type Sent, type SteerTone } from './steer';

export function Composer({ view, execution, onSent }: { view: RunView; execution: Execution; onSent: (sent: Sent | null) => void }) {
  if (observeOnly(view))
    return (
      <p className="composer-note" title={observeOnlyNote(view)}>
        <Info size={12} aria-hidden /> Messages to a running agent: not for an observed-only source
      </p>
    );
  if (!(view.capabilities ?? []).includes('steer'))
    return (
      <p className="composer-note" title={capabilityHint('steer')}>
        <Info size={12} aria-hidden /> Messages to a running agent: not supported by this Zeroshot
      </p>
    );
  return <SteerForm view={view} execution={execution} onSent={onSent} />;
}

function SteerForm({ view, execution, onSent }: { view: RunView; execution: Execution; onSent: (sent: Sent | null) => void }) {
  const [text, setText] = useState('');
  const [sending, setSending] = useState(false);
  const [status, setStatus] = useState<{ tone: SteerTone; text: string } | null>(null);
  const scope = steerScope(view);
  const send = async () => {
    const message = text.trim();
    if (!message || sending) return;
    setSending(true);
    setStatus(null);
    try {
      const result = await api.steer(view.target, view.runId, execution.id, message);
      setStatus(steerStatusText(result));
      if (result.status !== 'rejected') setText('');
      // Zeroshot 16.1 answers with a receipt: the message is sent; the chat shows when this
      // agent takes it in (at its next tool call or prompt).
      const sent = result.status === 'sent' ? `sent${scope === 'run' ? ', to all agents' : ''}` : result.status;
      onSent(result.status === 'rejected' ? null : { text: message, at: Date.now(), status: sent, reason: result.reason });
    } catch (cause) {
      setStatus({ tone: 'bad', text: cause instanceof ApiError ? cause.message : errorMessage(cause) });
    } finally {
      setSending(false);
    }
  };
  return (
    <form
      className="composer"
      onSubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
      <div className="composer-row">
        <label className="visually-hidden" htmlFor="composer-text">
          {scope === 'node' ? `Message ${execution.node}` : 'Message all agents in this run'}
        </label>
        <textarea
          id="composer-text"
          rows={2}
          value={text}
          disabled={sending}
          placeholder={scope === 'node' ? `Message ${execution.node} (Ctrl+Enter to send)` : 'Message all agents (Ctrl+Enter to send)'}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              void send();
            }
          }}
        />
        <button type="submit" className="button primary" disabled={sending || !text.trim()}>
          {sending ? <LoaderCircle size={14} className="spin" aria-hidden /> : <Send size={14} aria-hidden />} Send
        </button>
      </div>
      <p className="composer-note composer-scope">
        <Info size={12} aria-hidden /> {steerScopeNote(scope, execution.node)}
      </p>
      {status && (
        <p className={`composer-status tone-text-${status.tone}`} role="status">
          {status.text}
        </p>
      )}
    </form>
  );
}
