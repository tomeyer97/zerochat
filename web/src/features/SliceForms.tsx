/** The slice panel's inline forms, Retry with notes, Request changes and Edit, in one shell. */
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { LoaderCircle } from 'lucide-react';
import type { FeatureDetail, SliceView } from '../types';
import { editDiff, requestChangesBody, type EditBody } from '../features-model';

/**
 * A form inside the panel: its fields, then Cancel and the submit button (spinning while `busy`).
 * Escape cancels the form only, not the panel around it. A refused submit stays open: the panel
 * shows the error.
 */
function SliceForm({
  submitLabel,
  busy,
  canSubmit = true,
  onSubmit,
  onCancel,
  onChange,
  children,
}: {
  submitLabel: string;
  busy: boolean;
  canSubmit?: boolean;
  onSubmit: () => Promise<void>;
  onCancel: () => void;
  onChange?: () => void;
  children: ReactNode;
}) {
  return (
    <form
      className="slice-form"
      onSubmit={(e) => {
        e.preventDefault();
        void onSubmit().catch(() => {});
      }}
      onChange={onChange}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.stopPropagation();
          onCancel();
        }
      }}
    >
      {children}
      <div className="modal-buttons">
        <button type="button" className="button small" onClick={onCancel}>
          Cancel
        </button>
        <button type="submit" className="button small primary" disabled={busy || !canSubmit}>
          {busy && <LoaderCircle size={13} className="spin" aria-hidden />} {submitLabel}
        </button>
      </div>
    </form>
  );
}

export function RetryForm({
  slice,
  feature,
  busy,
  onSubmit,
  onCancel,
}: {
  slice: SliceView;
  feature: FeatureDetail;
  busy: boolean;
  onSubmit: (notes: string, fresh: boolean) => Promise<void>;
  onCancel: () => void;
}) {
  const [notes, setNotes] = useState('');
  const [fresh, setFresh] = useState(false);
  const when = runsWhen(feature);
  return (
    <SliceForm submitLabel={fresh ? 'Retry from scratch' : 'Retry'} busy={busy} onSubmit={() => onSubmit(notes, fresh)} onCancel={onCancel}>
      <label className="field">
        <span>Notes for the agent (optional)</span>
        <textarea rows={4} value={notes} autoFocus onChange={(e) => setNotes(e.target.value)} aria-describedby="retry-notes-help" />
        <span id="retry-notes-help" className="small muted">
          Added to this attempt of {slice.id} and every later one; they can't be removed here.
        </span>
      </label>
      <fieldset className="choice">
        <legend className="small">Start from</legend>
        <label className="check">
          <input type="radio" name="fresh" checked={!fresh} onChange={() => setFresh(false)} /> its worktree as it is (keeps the changes made so far)
        </label>
        <label className="check">
          <input type="radio" name="fresh" checked={fresh} onChange={() => setFresh(true)} /> scratch: a new worktree from the feature branch (the current one is discarded)
        </label>
      </fieldset>
      {when && <p className="small tone-text-warn">{when}</p>}
    </SliceForm>
  );
}

/** When a queued run starts: on unpause or start (nothing while the feature runs). */
function runsWhen(feature: FeatureDetail): string | null {
  if (feature.paused) return 'It runs when you unpause the feature.';
  if (!feature.active || feature.daemonPid == null) return 'It runs when the feature is started.';
  return null;
}

/** Request changes on a slice waiting for review: notes are required (they are the review). */
export function RequestChangesForm({
  slice,
  feature,
  busy,
  onSubmit,
  onCancel,
}: {
  slice: SliceView;
  feature: FeatureDetail;
  busy: boolean;
  onSubmit: (body: { action: 'retry'; notes: string }) => Promise<void>;
  onCancel: () => void;
}) {
  const [notes, setNotes] = useState('');
  const body = requestChangesBody(notes);
  const when = runsWhen(feature);
  return (
    <SliceForm submitLabel="Request changes" busy={busy} canSubmit={!!body} onSubmit={() => (body ? onSubmit(body) : Promise.resolve())} onCancel={onCancel}>
      <label className="field">
        <span>What to change</span>
        <textarea rows={4} value={notes} autoFocus required onChange={(e) => setNotes(e.target.value)} aria-describedby="changes-notes-help" />
        <span id="changes-notes-help" className="small muted">
          {slice.id} runs again in its worktree with these notes, then waits for your review again.
        </span>
      </label>
      {when && <p className="small tone-text-warn">{when}</p>}
    </SliceForm>
  );
}

export function EditForm({
  slice,
  feature,
  busy,
  onSubmit,
  onCancel,
  onChange,
}: {
  slice: SliceView;
  feature: FeatureDetail;
  busy: boolean;
  onSubmit: (body: EditBody) => Promise<void>;
  onCancel: () => void;
  onChange: () => void;
}) {
  const [title, setTitle] = useState(slice.title);
  const [deps, setDeps] = useState(slice.deps.join(', '));
  const [spec, setSpec] = useState(slice.spec ?? '');
  const titleRef = useRef<HTMLInputElement>(null);
  useEffect(() => titleRef.current?.focus(), []);
  const ids = feature.slices.map((s) => s.id);
  const edit = editDiff(slice, { title, deps, spec }, ids);
  const others = feature.slices.filter((s) => s.id !== slice.id).map((s) => `${s.id} ${s.title}`);
  return (
    <SliceForm submitLabel="Save" busy={busy} canSubmit={edit.changed && !edit.depsError && !edit.titleError} onSubmit={() => onSubmit(edit.body)} onCancel={onCancel} onChange={onChange}>
      <label className="field">
        <span>Title</span>
        <input ref={titleRef} value={title} onChange={(e) => setTitle(e.target.value)} aria-invalid={!!edit.titleError || undefined} />
        {edit.titleError && <span className="small tone-text-bad">{edit.titleError}</span>}
      </label>
      <label className="field">
        <span>After (slice ids)</span>
        <input value={deps} onChange={(e) => setDeps(e.target.value)} placeholder="01, 02" aria-invalid={!!edit.depsError || undefined} />
        {edit.depsError ? <span className="small tone-text-bad">{edit.depsError}</span> : <span className="small muted">{others.join(' · ')}</span>}
      </label>
      {slice.spec != null && (
        <label className="field">
          <span>Spec</span>
          <textarea rows={10} className="mono" value={spec} onChange={(e) => setSpec(e.target.value)} />
          {edit.mentionsDeps && <span className="small tone-text-warn">The spec still has its own “Depends on” line; the agent reads it as written.</span>}
        </label>
      )}
    </SliceForm>
  );
}
