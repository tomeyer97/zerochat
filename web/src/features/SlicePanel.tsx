/**
 * The slice panel: one slice of a feature beside the graph. Its header says where the slice
 * stands and offers what you can do with it (approve, retry, merge its worktree by hand, edit,
 * skip; zsf, "Zeroshot features", decides which; a slice waiting for review gets exactly Approve,
 * Request changes and Skip); its body has the inline forms and the sections.
 */
import { useEffect, useRef, useState, type ReactNode, type Ref } from 'react';
import { LoaderCircle, X } from 'lucide-react';
import { api, ApiError, errorMessage } from '../api';
import type { FeatureDetail, SliceView } from '../types';
import {
  acceptSummary,
  dependentsOf,
  depMark,
  isActiveSlice,
  reviewActions,
  shortDuration,
  skipConsequences,
  sliceDuration,
  sliceStateSentence,
  sliceStateText,
  sliceTone,
  APPROVAL_SLICE,
  APPROVAL_SLICE_HINT,
} from '../features-model';
import { costText, timeOfDay } from '../format';
import { ErrorBox } from '../common';
import { withCode } from '../onboarding/Glossary';
import { ConfirmDialog } from '../components/ConfirmDialog';
import { ActionButton } from './ActionButton';
import { EditForm, RequestChangesForm, RetryForm } from './SliceForms';
import { SliceSections } from './SliceSections';

type Dialog = 'skip' | 'accept' | null;
type Form = 'retry' | 'changes' | 'edit' | null;
type ActionBody = Parameters<typeof api.sliceAction>[2];

/** The Merge worktree dialog lists this many files, then "and N more". */
const ACCEPT_FILES_SHOWN = 12;

export function SlicePanel({
  feature,
  slice,
  now,
  onClose,
  onOpen,
  onDone,
  onRefused,
}: {
  feature: FeatureDetail;
  slice: SliceView;
  now: number;
  onClose: () => void;
  onOpen: (id?: string) => void;
  onDone: (message: string) => void;
  onRefused: () => void;
}) {
  const [dialog, setDialog] = useState<Dialog>(null);
  const [form, setForm] = useState<Form>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  // Opened from a card or a link on the page (not by loading the URL): focus follows into the panel.
  useEffect(() => {
    if (document.activeElement?.closest('.slice-graph, .slice-list, .attention-list, .slice-deps, .feature-now-parts, .slice-panel')) heading.current?.focus();
  }, []);
  // A form for an action the slice no longer offers (someone else acted) closes.
  const canRetry = slice.actions.includes('retry');
  const canEdit = slice.actions.includes('edit');
  useEffect(() => {
    if (((form === 'retry' || form === 'changes') && !canRetry) || (form === 'edit' && !canEdit)) setForm(null);
  }, [form, canRetry, canEdit]);
  /** Sends an action; refused because the slice moved on (409), the panel closes its form and reloads. */
  const act = async (body: ActionBody) => {
    setBusy(body.action);
    setError(null);
    try {
      const result = await api.sliceAction(feature.name, slice.id, body);
      setForm(null);
      setDialog(null);
      onDone(result.message);
      heading.current?.focus();
    } catch (cause) {
      setError(errorMessage(cause));
      if (cause instanceof ApiError && cause.status === 409) {
        setForm(null);
        setDialog(null);
        onRefused();
      }
      throw cause;
    } finally {
      setBusy(null);
    }
  };
  const dependents = dependentsOf(feature.slices, slice.id);
  const openDialog = (which: Dialog) => {
    setForm(null);
    setDialog(which);
  };
  return (
    <aside className="node-panel slice-panel" aria-label={`Slice ${slice.id}`}>
      <SliceHeader feature={feature} slice={slice} now={now} heading={heading} onClose={onClose} onOpen={onOpen}>
        {slice.actions.length > 0 && (
          <SliceActions
            slice={slice}
            dependents={dependents}
            busy={busy}
            form={form}
            onAct={(body) => void act(body).catch(() => {})}
            onToggleForm={(which) => setForm(form === which ? null : which)}
            onDialog={openDialog}
          />
        )}
        {error && <ErrorBox compact message={error} />}
      </SliceHeader>
      <div className="panel-body">
        <div className="panel-pad slice-sections">
          {form === 'retry' && <RetryForm slice={slice} feature={feature} busy={busy === 'retry'} onSubmit={(notes, fresh) => act({ action: 'retry', notes, fresh })} onCancel={() => setForm(null)} />}
          {form === 'changes' && <RequestChangesForm slice={slice} feature={feature} busy={busy === 'retry'} onSubmit={act} onCancel={() => setForm(null)} />}
          {form === 'edit' && (
            <EditForm
              slice={slice}
              feature={feature}
              busy={busy === 'edit'}
              onChange={() => setError(null)}
              onSubmit={(body) => act({ action: 'edit', ...body })}
              onCancel={() => {
                setForm(null);
                setError(null);
              }}
            />
          )}
          <SliceSections feature={feature} slice={slice} now={now} showSpec={form !== 'edit'} />
        </div>
      </div>
      <SliceDialogs dialog={dialog} slice={slice} dependents={dependents} onClose={() => setDialog(null)} onConfirm={act} />
    </aside>
  );
}

/** Title, status, the sentence on where it stands, what it comes after, its facts; then `children` (the actions). */
function SliceHeader({
  feature,
  slice,
  now,
  heading,
  onClose,
  onOpen,
  children,
}: {
  feature: FeatureDetail;
  slice: SliceView;
  now: number;
  heading: Ref<HTMLHeadingElement>;
  onClose: () => void;
  onOpen: (id?: string) => void;
  children: ReactNode;
}) {
  const tone = sliceTone(slice.status);
  // Only what needs a person keeps its colour: merged and pending read as plain text.
  const sentenceTone = slice.status === 'pending' || tone === 'ok' ? 'muted' : tone;
  const daemonUp = feature.daemonPid != null;
  const sentence = sliceStateSentence(slice, { slices: feature.slices, paused: !!feature.paused, daemonUp, now, pollSecs: feature.pollSecs, feature: feature.name });
  return (
    <header className="panel-header">
      <div className="panel-title-row">
        <h2 className="panel-title" ref={heading} tabIndex={-1}>
          <span className="muted tabular">{slice.id}</span> {slice.title}
        </h2>
        <span className={`pill small tone-${tone}`}>
          {isActiveSlice(slice.status) && <LoaderCircle size={12} className="spin" aria-hidden />}
          {sliceStateText(slice, now, daemonUp, feature.pollSecs)}
        </span>
        <button type="button" className="icon-button panel-close" aria-label="Close (Esc)" title="Close (Esc)" onClick={onClose}>
          <X size={17} />
        </button>
      </div>
      <p className={`slice-state tone-text-${sentenceTone}`}>{withCode(sentence)}</p>
      {slice.deps.length > 0 && <SliceDeps feature={feature} slice={slice} onOpen={onOpen} />}
      <SliceFacts slice={slice} now={now} />
      {children}
    </header>
  );
}

/** `after 01 ✓, 02 (failed)`: each dependency, a link to its panel, with its state. */
function SliceDeps({ feature, slice, onOpen }: { feature: FeatureDetail; slice: SliceView; onOpen: (id?: string) => void }) {
  const statusOf = (id: string) => feature.slices.find((s) => s.id === id)?.status;
  return (
    <p className="small slice-deps">
      after{' '}
      {slice.deps.map((d, i) => {
        const mark = depMark(statusOf(d));
        return (
          <span key={d}>
            {i > 0 && ', '}
            <button type="button" className="text-button inline-link" onClick={() => onOpen(d)}>
              {d}
            </button>
            <span className={`muted dep-state dep-${mark.tone}`}> {mark.text}</span>
          </span>
        );
      })}
    </p>
  );
}

/** Approval slice, time, cost and leases, in a row. */
function SliceFacts({ slice, now }: { slice: SliceView; now: number }) {
  const facts: ReactNode[] = [];
  if (slice.checkpoint) {
    const approvedAt = slice.approvedAt ? ` ${timeOfDay(slice.approvedAt, now)}` : '';
    facts.push(
      <span key="c" title={APPROVAL_SLICE_HINT}>
        {APPROVAL_SLICE}
        {slice.approved ? ` · approved${approvedAt}` : ''}
      </span>
    );
  }
  const spent = sliceDuration(slice, now);
  if (spent != null) {
    facts.push(
      <span key="t" title="Time its runs took">
        {shortDuration(spent)}
      </span>
    );
  }
  if (slice.costUsd != null) facts.push(<span key="$">{costText(slice.costUsd, slice.costEstimated)}</span>);
  if (slice.leases.length) facts.push(<span key="l">needs {slice.leases.map((l) => `lease ${l}`).join(', ')}</span>);
  return facts.length > 0 ? <p className="panel-facts">{facts}</p> : null;
}

/**
 * The actions the slice offers, in a fixed order. Approve and Retry act at once; Retry with
 * notes and Edit open a form below; Merge worktree and Skip ask first in a dialog.
 */
function SliceActions({
  slice,
  dependents,
  busy,
  form,
  onAct,
  onToggleForm,
  onDialog,
}: {
  slice: SliceView;
  dependents: SliceView[];
  busy: string | null;
  form: Form;
  onAct: (body: ActionBody) => void;
  onToggleForm: (form: Form) => void;
  onDialog: (dialog: Dialog) => void;
}) {
  const review = reviewActions(slice);
  if (review) return <ReviewActions actions={review} busy={busy} form={form} onAct={onAct} onToggleForm={onToggleForm} onDialog={onDialog} />;
  const has = (a: SliceView['actions'][number]) => slice.actions.includes(a);
  const approveAlso = dependents.length ? ` and start ${dependents.map((d) => d.id).join(', ')}` : '';
  return (
    <div className="slice-actions">
      {has('approve') && (
        <ActionButton name="approve" busy={busy} primary onClick={() => onAct({ action: 'approve' })}>
          Approve{approveAlso}
        </ActionButton>
      )}
      {has('retry') && (
        <>
          {/* Its spinner is the form's while the form sends the retry. */}
          <ActionButton name="retry" busy={busy} spinning={busy === 'retry' && form !== 'retry'} primary onClick={() => onAct({ action: 'retry' })} title="Run it again in the same worktree">
            Retry
          </ActionButton>
          <FormToggle open={form === 'retry'} onToggle={() => onToggleForm('retry')}>
            Retry with notes…
          </FormToggle>
        </>
      )}
      {has('accept') && (
        <button type="button" className="button small" onClick={() => onDialog('accept')} title="Merge the worktree as it is now, without the gate (after you fixed it by hand)">
          Merge worktree…
        </button>
      )}
      {has('edit') && (
        <FormToggle open={form === 'edit'} onToggle={() => onToggleForm('edit')}>
          Edit…
        </FormToggle>
      )}
      {has('skip') && (
        <button type="button" className="button small danger" onClick={() => onDialog('skip')}>
          Skip…
        </button>
      )}
    </div>
  );
}

/** A slice waiting for review: Approve (checks, then merges), Request changes (notes form), Skip (asks first). */
function ReviewActions({
  actions,
  busy,
  form,
  onAct,
  onToggleForm,
  onDialog,
}: {
  actions: NonNullable<ReturnType<typeof reviewActions>>;
  busy: string | null;
  form: Form;
  onAct: (body: ActionBody) => void;
  onToggleForm: (form: Form) => void;
  onDialog: (dialog: Dialog) => void;
}) {
  return (
    <div className="slice-actions">
      {/* biome-ignore lint/suspicious/useIterableCallbackReturn: the switch covers every `opens` value, so every path returns. */}
      {actions.map((a) => {
        switch (a.opens) {
          case 'now':
            return (
              <ActionButton key={a.action} name={a.action} busy={busy} primary onClick={() => onAct({ action: a.action })}>
                {a.label}
              </ActionButton>
            );
          case 'form':
            return (
              <FormToggle key={a.action} open={form === 'changes'} onToggle={() => onToggleForm('changes')}>
                {a.label}
              </FormToggle>
            );
          case 'dialog':
            return (
              <button key={a.action} type="button" className="button small danger" onClick={() => onDialog('skip')}>
                {a.label}
              </button>
            );
        }
      })}
    </div>
  );
}

/** A button that opens (and, pressed again, closes) one of the panel's inline forms. */
function FormToggle({ open, onToggle, children }: { open: boolean; onToggle: () => void; children: ReactNode }) {
  return (
    <button type="button" className={`button small ${open ? 'on' : ''}`} aria-expanded={open} onClick={onToggle}>
      {children}
    </button>
  );
}

/** The confirmations of Skip and Merge worktree. */
function SliceDialogs({
  dialog,
  slice,
  dependents,
  onClose,
  onConfirm,
}: {
  dialog: Dialog;
  slice: SliceView;
  dependents: SliceView[];
  onClose: () => void;
  onConfirm: (body: ActionBody) => Promise<void>;
}) {
  if (dialog === 'skip') {
    return (
      <ConfirmDialog title={`Skip ${slice.id}?`} confirmLabel={`Skip ${slice.id}`} danger onClose={onClose} onConfirm={() => onConfirm({ action: 'skip' })}>
        <p>{skipConsequences(slice, dependents)}</p>
      </ConfirmDialog>
    );
  }
  if (dialog === 'accept') {
    const files = slice.worktreeChanges?.files ?? [];
    return (
      <ConfirmDialog title={`Merge ${slice.id} without the gate?`} confirmLabel="Merge without gate" onClose={onClose} onConfirm={() => onConfirm({ action: 'accept' })}>
        <p>{acceptSummary(slice)}</p>
        {slice.worktreeChanges && (
          <ul className="file-stat-list small">
            {files.slice(0, ACCEPT_FILES_SHOWN).map((f) => (
              <li key={f.path}>
                <span className="mono">{f.path}</span>{' '}
                <span className="muted tabular">
                  +{f.additions} −{f.deletions}
                </span>
              </li>
            ))}
            {files.length > ACCEPT_FILES_SHOWN && <li className="muted">and {files.length - ACCEPT_FILES_SHOWN} more</li>}
          </ul>
        )}
      </ConfirmDialog>
    );
  }
  return null;
}
