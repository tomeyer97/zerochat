import { useMemo, useState } from 'react';
import { AlertTriangle, Info, LoaderCircle, Play, RotateCcw } from 'lucide-react';
import { api, errorMessage } from './api';
import type { Checkpoint, GraphNodeInfo, ReplayBody, RunView } from './types';
import {
  ADVANCED_FIELDS,
  advancedJson,
  applyEdits,
  checkpointIteration,
  checkpointRound,
  editableNodes,
  failedHereNote,
  nodeEditChanged,
  originalValues,
  preselectCheckpoint,
  replayModels,
  replayRequest,
  replayVerbs,
  rollbackText,
  startAgainStep,
  validationLines,
  validationSummary,
  withLastRoundBump,
  type AdvancedJson,
  type NodeEdit,
  type ReplayNotice,
  type StartStage,
} from './replay-model';
import type { ReplayFocus } from './RunSummary';
import { GROUP_KINDS, humanId, isGroupKind } from './graph-model';
import { fullTime, offsetText, prettyJson } from './format';
import { useResource } from './hooks';
import { ErrorBox, InfoBanner, Modal, Skeleton } from './common';
import { capabilityHint } from './status';
import { unchangedWarning } from './failure-class';
import { Markdown } from './Markdown';
import { ConfirmDialog } from './components/ConfirmDialog';

type Busy = 'validate' | 'start' | null;

export function ReplayDialog({ view, focus = null, onClose, onStarted }: { view: RunView; focus?: ReplayFocus; onClose: () => void; onStarted: (runId: string | null, message: string) => void }) {
  const canEdit = (view.capabilities ?? []).includes('replay');
  const checkpoints = useResource<{ checkpoints: Checkpoint[] }>(`${view.target}/${view.runId}/checkpoints`, (signal) => api.checkpoints(view.target, view.runId, signal));
  const sorted = useMemo(() => [...(checkpoints.data?.checkpoints ?? [])].sort((a, b) => a.sequence - b.sequence), [checkpoints.data]);
  const preselected = useMemo(() => preselectCheckpoint(sorted, view), [sorted, view]);
  const [chosenId, setChosenId] = useState<string | null>(null);
  const chosen = sorted.find((c) => c.checkpointId === chosenId) ?? preselected;
  // The new run keeps the title; its header says it is a replay.
  const [title, setTitle] = useState(view.title || '');
  const [userEdits, setEdits] = useState<Record<string, NodeEdit>>({});
  const [advanced, setAdvanced] = useState<AdvancedJson | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [result, setResult] = useState<ReplayNotice | null>(null);
  // Starting again rolls the workspace back: the footer's button asks, the confirm step sends.
  const [stage, setStage] = useState<StartStage>('choose');
  const [checked, setChecked] = useState<ReplayBody | null>(null);
  const chosenLabel = chosen ? checkpointLabel(chosen, view.graphNodes) : null;
  const verbs = replayVerbs(canEdit, null, chosenLabel);
  const nodes = useMemo(() => editableNodes(chosen?.node, view.graphNodes), [chosen?.node, view.graphNodes]);
  const loops = useMemo(() => {
    const names = new Set(nodes.flatMap((n) => n.loops));
    return view.graphNodes.filter((n) => n.kind === 'loop' && names.has(n.name));
  }, [nodes, view.graphNodes]);
  const models = useMemo(() => replayModels(view.runtime, view.executions), [view.runtime, view.executions]);
  const original = originalValues(view.graphNodes);
  const edit = (name: string, change: NodeEdit) => setEdits((all) => ({ ...all, [name]: { ...all[name], ...change } }));
  const lastRound = (chosen && checkpointRound(chosen, view.graphNodes)) || null;
  const edits = useMemo(
    () => (canEdit ? withLastRoundBump(userEdits, lastRound) : userEdits),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [canEdit, lastRound?.loop, lastRound?.max, userEdits]
  );
  const otherLoops = loops.filter((l) => l.name !== lastRound?.loop);

  const request = (validateOnly: boolean) =>
    replayRequest({
      view,
      checkpointId: chosen?.checkpointId,
      edits: canEdit ? { nodes: edits, original, title } : null,
      advanced,
      validateOnly,
    });

  /** Validates; `start` then asks to confirm, and the confirm starts the replay. */
  const submit = async (start: boolean) => {
    const check = request(true);
    if (!('checkpointId' in check)) {
      setResult(check);
      return;
    }
    setBusy(start ? 'start' : 'validate');
    setResult(null);
    try {
      const response = await api.replay(view.target, view.runId, check);
      const problems = validationLines(response);
      if (response.valid === false || problems.length) {
        setResult({ tone: 'bad', lines: problems.length ? problems : ['Zeroshot rejected this replay.'] });
        return;
      }
      if (!start) {
        setResult({ tone: 'ok', lines: response.mode === 'resume' ? ['This checkpoint can be resumed.'] : validationSummary(response) });
        return;
      }
      setChecked(check);
      setStage(await startAgainStep(stage, 'ask', async () => {}));
    } catch (cause) {
      setResult({ tone: 'bad', lines: [errorMessage(cause)] });
    } finally {
      setBusy(null);
    }
  };

  /** The confirmed start: the checked replay with changes, or the unchanged start from the checkpoint. */
  const send = async () => {
    if (canEdit) {
      if (!checked) return;
      const started = await api.replay(view.target, view.runId, { ...checked, validateOnly: false });
      const runId = typeof started.runId === 'string' ? started.runId : null;
      onStarted(runId, runId ? verbs.started : `Zeroshot answered: ${prettyJson(started)}`);
      return;
    }
    if (!chosen) return;
    const response = await api.resume(view.target, view.runId, chosen.checkpointId);
    const runId = typeof response.runId === 'string' ? response.runId : null;
    onStarted(runId && runId !== view.runId ? runId : null, verbs.started);
  };

  if (stage === 'confirm' && chosen && chosenLabel) {
    // Cancel (or Escape) goes back to the checkpoint list; nothing was sent.
    return (
      <ConfirmDialog
        title={verbs.confirmTitle}
        confirmLabel={verbs.confirm}
        danger
        onClose={() => void startAgainStep(stage, 'cancel', send).then(setStage)}
        onConfirm={async () => setStage(await startAgainStep(stage, 'confirm', send))}
      >
        <p>{rollbackText(chosenLabel)}</p>
      </ConfirmDialog>
    );
  }

  const none = !!checkpoints.data && !sorted.length;
  const footer = (
    <ReplayFooter
      result={result}
      none={none}
      canEdit={canEdit}
      disabled={!chosen || !!busy}
      busy={busy}
      onClose={onClose}
      onCheck={() => void submit(false)}
      onStart={() => void submit(true)}
      onResume={() => {
        setResult(null);
        void startAgainStep(stage, 'ask', send).then(setStage);
      }}
      next={verbs.next}
    />
  );

  // Without changes, a failure the settings caused (unknown model, a timeout to raise) comes back.
  const warning = !canEdit && view.status === 'failed' && view.failure ? unchangedWarning(view.failure) : null;
  return (
    <Modal title={verbs.title} onClose={onClose} footer={footer} wide>
      {warning && (
        <InfoBanner tone="warn" icon={<AlertTriangle size={15} aria-hidden />}>
          <Markdown text={warning} inline />
        </InfoBanner>
      )}
      {!canEdit && !none && !warning && (
        <InfoBanner icon={<Info size={15} aria-hidden />}>
          <span title={capabilityHint('replay')}>A new run from the checkpoint you pick, with the same graph, prompts and models.</span>
        </InfoBanner>
      )}
      {checkpoints.loading && <Skeleton lines={4} />}
      {checkpoints.error && <ErrorBox title="Could not list checkpoints" message={checkpoints.error} onRetry={checkpoints.reload} />}
      {none && <p>This run saved no checkpoints, so it cannot start again from a node.</p>}
      {sorted.length > 0 && (
        <>
          <h3 className="dialog-section">Start before</h3>
          <CheckpointList view={view} checkpoints={sorted} chosen={chosen} preselected={preselected} onChoose={setChosenId} />
          {canEdit && lastRound && (
            <label className="field inline" title={`Max rounds of ${lastRound.loop}. This checkpoint is in its last round (${lastRound.round} of ${lastRound.max}), so it starts one higher.`}>
              <span>
                Max rounds <span className="muted">({lastRound.max} used)</span>
              </span>
              <input
                type="number"
                min={1}
                inputMode="numeric"
                disabled={!!advanced}
                value={edits[lastRound.loop]?.maxRounds ?? ''}
                onChange={(e) => edit(lastRound.loop, { maxRounds: e.target.value })}
              />
            </label>
          )}
        </>
      )}
      {canEdit && chosen && (
        <details className="replay-changes" open={!!focus}>
          <summary>Change settings</summary>
          <label className="field">
            <span>Title of the new run</span>
            <input value={title} onChange={(e) => setTitle(e.target.value)} />
          </label>
          {advanced && <p className="field-note">The JSON under Advanced replaces these fields.</p>}
          <fieldset className="node-edits" disabled={!!advanced}>
            {otherLoops.map((l) => (
              <label key={l.name} className="field inline">
                <span>
                  Max rounds of <strong>{l.name}</strong>
                </span>
                <input type="number" min={1} inputMode="numeric" value={edits[l.name]?.maxRounds ?? String(l.maxIterations ?? '')} onChange={(e) => edit(l.name, { maxRounds: e.target.value })} />
              </label>
            ))}
            {nodes.map((n) => (
              <NodeEditFields key={n.name} node={n} edit={edits[n.name] ?? {}} focus={focus} onEdit={(change) => edit(n.name, change)} />
            ))}
            {nodes.length === 0 && <p className="muted small">No agent node runs again from this checkpoint.</p>}
            <datalist id="replay-models">
              {models.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
          </fieldset>
          <AdvancedJsonFields
            advanced={advanced}
            result={result}
            onOpen={() => setAdvanced(advancedJson(view, applyEdits(view.graph, view.runtime, edits, original), prettyJson))}
            onChange={setAdvanced}
          />
        </details>
      )}
    </Modal>
  );
}

/** The same name the graph and the button already use for this node: a loop/parallel/map group
 * by its caption ("In parallel"), an agent by its humanised name. */
function checkpointNodeLabel(nodeName: string, graphNodes: GraphNodeInfo[]): string {
  const kind = graphNodes.find((n) => n.name === nodeName)?.kind;
  return isGroupKind(kind) ? GROUP_KINDS[kind].caption : humanId(nodeName);
}

/** A checkpoint as the confirm step names it: `In parallel (round 2)`. */
function checkpointLabel(c: Checkpoint, graphNodes: GraphNodeInfo[]): string {
  const round = checkpointIteration(c);
  return `${checkpointNodeLabel(c.node, graphNodes)}${round != null ? ` (round ${round})` : ''}`;
}

function CheckpointList({
  view,
  checkpoints,
  chosen,
  preselected,
  onChoose,
}: {
  view: RunView;
  checkpoints: Checkpoint[];
  chosen: Checkpoint | undefined;
  preselected: Checkpoint | undefined;
  onChoose: (checkpointId: string) => void;
}) {
  return (
    <ul className="checkpoint-list" role="radiogroup" aria-label="Checkpoints">
      {checkpoints.map((c) => {
        const round = checkpointIteration(c);
        const selected = c.checkpointId === chosen?.checkpointId;
        const failedHere = !!view.failure && c.checkpointId === preselected?.checkpointId;
        return (
          <li key={c.checkpointId}>
            <label className={`checkpoint ${selected ? 'selected' : ''}`}>
              <input type="radio" name="checkpoint" checked={selected} onChange={() => onChoose(c.checkpointId)} />
              <span className="cp-node">
                <strong>{checkpointNodeLabel(c.node, view.graphNodes)}</strong>
                {round != null && <span className="muted"> · round {round}</span>}
                {c.mapIndices?.length > 0 && <span className="muted"> · item {c.mapIndices.join('.')}</span>}
                {failedHere && <span className="muted"> · {failedHereNote(view)}</span>}
              </span>
              <span className="muted small cp-time" title={`Checkpoint taken ${fullTime(c.createdAt)}`}>
                checkpoint {offsetText(c.createdAt, view.startedAt)}
              </span>
            </label>
          </li>
        );
      })}
    </ul>
  );
}

/** One agent's model, timeout and instructions, folded under its name. */
function NodeEditFields({ node: n, edit: e, focus, onEdit }: { node: GraphNodeInfo; edit: NodeEdit; focus: ReplayFocus; onEdit: (change: NodeEdit) => void }) {
  const focused = focus?.node === n.name;
  return (
    <details className="node-edit" open={focused}>
      <summary>
        <strong>{n.name}</strong> <span className="muted node-edit-model">{e.model ?? n.model ?? 'no model'}</span>
        {nodeEditChanged(e, n) && <span className="badge">changed</span>}
      </summary>
      <label className="field">
        <span>Model</span>
        <input
          list="replay-models"
          title="Model names are not checked until the run starts."
          autoFocus={focused && focus?.field === 'model'}
          value={e.model ?? n.model ?? ''}
          onChange={(ev) => onEdit({ model: ev.target.value })}
        />
      </label>
      <label className="field inline">
        <span>Timeout (seconds)</span>
        <input
          type="number"
          min={1}
          inputMode="numeric"
          autoFocus={focused && focus?.field === 'timeout'}
          placeholder="none"
          value={e.timeout ?? (n.timeoutMs != null ? String(n.timeoutMs / 1000) : '')}
          onChange={(ev) => onEdit({ timeout: ev.target.value })}
        />
      </label>
      <label className="field">
        <span>Instructions</span>
        <textarea rows={6} value={e.instructions ?? n.instructions ?? ''} onChange={(ev) => onEdit({ instructions: ev.target.value })} />
      </label>
    </details>
  );
}

/** The graph, runtime and input as JSON, filled in when first opened; a field Zeroshot or parsing rejected is marked. */
function AdvancedJsonFields({ advanced, result, onOpen, onChange }: { advanced: AdvancedJson | null; result: ReplayNotice | null; onOpen: () => void; onChange: (next: AdvancedJson) => void }) {
  return (
    <details
      className="advanced"
      onToggle={(e) => {
        if ((e.target as HTMLDetailsElement).open && !advanced) onOpen();
      }}
    >
      <summary>Advanced: graph, runtime, input JSON</summary>
      <p className="muted small">Before the checkpoint only instructions and timeouts may change.</p>
      {advanced &&
        ADVANCED_FIELDS.map(({ key, label }) => {
          const error = result?.field === key ? result.lines.join(' ') : null;
          return (
            <label key={key} className={`field ${error != null ? 'invalid' : ''}`}>
              <span>{label}</span>
              <textarea className="mono" rows={10} spellCheck={false} value={advanced[key]} onChange={(e) => onChange({ ...advanced, [key]: e.target.value })} aria-invalid={error != null} />
              {error != null && <span className="field-error">{error}</span>}
            </label>
          );
        })}
    </details>
  );
}

/** The last answer (unless it belongs to an Advanced field) and the buttons: check and start, or start again. */
function ReplayFooter({
  result,
  none,
  canEdit,
  disabled,
  busy,
  onClose,
  onCheck,
  onStart,
  onResume,
  next,
}: {
  result: ReplayNotice | null;
  /** The run saved no checkpoints: only Close. */
  none: boolean;
  canEdit: boolean;
  disabled: boolean;
  busy: Busy;
  onClose: () => void;
  onCheck: () => void;
  onStart: () => void;
  onResume: () => void;
  /** The button that leads to the confirm step (the dialog's verb). */
  next: string;
}) {
  return (
    <>
      {result && !result.field && (
        <div className={`replay-result ${result.tone === 'bad' ? 'tone-text-bad' : 'muted'}`} role="status">
          <div>
            {result.lines.map((l, i) => (
              <p key={i}>{l}</p>
            ))}
          </div>
        </div>
      )}
      <div className="modal-buttons">
        <button type="button" className="button" onClick={onClose}>
          {none ? 'Close' : 'Cancel'}
        </button>
        {none ? null : canEdit ? (
          <>
            <button type="button" className="button" disabled={disabled} onClick={onCheck} title="Ask Zeroshot what this replay would reuse, without starting it">
              {busy === 'validate' && <LoaderCircle size={14} className="spin" aria-hidden />} Check
            </button>
            <button type="button" className="button primary" disabled={disabled} onClick={onStart} title="Checks the settings with Zeroshot, then asks to confirm">
              {busy === 'start' ? <LoaderCircle size={14} className="spin" aria-hidden /> : <Play size={14} aria-hidden />} {next}
            </button>
          </>
        ) : (
          <button type="button" className="button primary" disabled={disabled} onClick={onResume}>
            {busy === 'start' ? <LoaderCircle size={14} className="spin" aria-hidden /> : <RotateCcw size={14} aria-hidden />} {next}
          </button>
        )}
      </div>
    </>
  );
}
