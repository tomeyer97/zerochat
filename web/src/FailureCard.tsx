import { useState } from 'react';
import { Info, Play, RotateCcw } from 'lucide-react';
import type { Failure, FailureAction, Overview, RunView } from './types';
import { observeOnly, observeOnlyNote } from './status';
import { duration, timeOfDay, untilText } from './format';
import { featureHref, nodeHref, visitOf } from './route';
import { CopyButton } from './common';
import { Markdown } from './Markdown';
import { RunLinkTo } from './RunHeader';
import { actionsNote, resetNote, unchangedFails } from './failure-class';
import { replayVerbs } from './replay-model';
import { effectiveAction, failureModel, primaryLabel, replayFocus, resumeLabel, type ReplayFocus } from './run-model';

type FailureFacts = ReturnType<typeof failureModel>;

export function FailureCard({
  failure,
  view,
  now,
  overview,
  onReplay,
  onResume,
  panelOpen = false,
}: {
  failure: Failure;
  view: RunView;
  now: number;
  overview: Overview | undefined;
  onReplay: (focus: ReplayFocus) => void;
  onResume: () => void;
  /** A node panel is open; it shows the reviewer's feedback in full, so the card keeps one line. */
  panelOpen?: boolean;
}) {
  const model = failureModel(failure, view, now);
  const { info } = model;
  const when = (
    <p className="failure-when">
      <FailureWhen failure={failure} view={view} model={model} />
    </p>
  );
  const actions = <FailureActions failure={failure} view={view} model={model} now={now} overview={overview} onReplay={onReplay} onResume={onResume} />;
  const raw = failure.lastErrors.length > 0 && (
    <details className="failure-raw">
      <summary>Error output</summary>
      <pre>{failure.lastErrors.join('\n')}</pre>
    </details>
  );

  if (model.stopped) {
    // A stop someone asked for is not a failure: one line, the actions, nothing to explain.
    return (
      <section className="failure-card compact" aria-label="Stopped" title={model.titleTip}>
        {when}
        {actions}
        {raw}
      </section>
    );
  }
  return (
    <section className={`failure-card run-${view.status}`} aria-labelledby="failure-title">
      <p className={`failure-class${info.tone ? ` tone-text-${info.tone}` : ''}`} title={info.explain}>
        {info.label}
      </p>
      <h2 id="failure-title" className="failure-title" title={model.titleTip}>
        {failure.title}
      </h2>
      {failure.detail && <p className="failure-detail">{failure.detail}</p>}
      {failure.fix && (
        <p className="failure-fix">
          <strong>Fix:</strong> <Markdown text={failure.fix} inline />
          {failure.fixCommand && (
            <span className="fix-command">
              <code>{failure.fixCommand}</code>
              <CopyButton text={failure.fixCommand} label="Copy command" />
            </span>
          )}
        </p>
      )}
      {model.latestRound.length > 0 && (
        <div className={`feedback ${panelOpen ? 'one-line' : ''}`}>
          {model.latestRound.map((f) => (
            <div key={f.execution} className="feedback-item">
              <div className="feedback-head" title={f.verdict ? `Verdict: ${f.verdict}` : undefined}>
                <a href={nodeHref(view.target, view.runId, f.node, { round: f.round })}>
                  {f.node} · round {f.round} ›
                </a>
              </div>
              {f.text ? <ClampedMarkdown text={f.text} /> : <p className="muted small">No feedback text was recorded.</p>}
            </div>
          ))}
        </div>
      )}
      {when}
      {actions}
      {raw}
    </section>
  );
}

/** When it failed (or stopped): the node, how long it ran and how far into the run, what was cut
 * off, when the run closed and which nodes were done before it. */
function FailureWhen({ failure, view, model }: { failure: Failure; view: RunView; model: FailureFacts }) {
  const { culprit, stopped, stalled, execMs, interrupted } = model;
  const nodeLink = failure.node ? nodeHref(view.target, view.runId, failure.node, culprit ? visitOf(culprit) : {}) : null;
  return (
    <span className="failure-when-text" title={model.whenTip}>
      {!stopped && !stalled && nodeLink && failure.feedback.length === 0 ? (
        <>
          <a href={nodeLink}>{failure.node}</a> failed
        </>
      ) : (
        model.verb
      )}
      {execMs != null ? (
        <>
          {' '}
          after {duration(execMs)}
          {model.intoRunDiffers && <> ({duration(failure.intoRunMs)} into the run)</>}
        </>
      ) : (
        failure.intoRunMs != null && <> at {duration(failure.intoRunMs)}</>
      )}
      {interrupted.length > 0 && (
        <span className="muted">
          {' · interrupted: '}
          {interrupted.map((e, i) => [
            i > 0 ? ', ' : '',
            <a key={e.id} href={nodeHref(view.target, view.runId, e.node, visitOf(e))}>
              {e.node}
            </a>,
          ])}
        </span>
      )}
      {failure.closedAfterMs != null && (
        <span className="muted">
          {' '}
          · run closed {duration(failure.closedAfterMs)} later{failure.closedByPostmortem ? ' (postmortem)' : ''}
        </span>
      )}
      {failure.kept.length > 0 && <span className="muted"> · done before it: {failure.kept.join(', ')}</span>}
    </span>
  );
}

/** What can be done next: open the slice (a feature slice's run), nothing (an observe-only
 * source), or resume and replay. */
function FailureActions({
  failure,
  view,
  model,
  now,
  overview,
  onReplay,
  onResume,
}: {
  failure: Failure;
  view: RunView;
  model: FailureFacts;
  now: number;
  overview: Overview | undefined;
  onReplay: (focus: ReplayFocus) => void;
  onResume: () => void;
}) {
  const { slice } = model;
  const resumedAs = (prefix: string) =>
    view.resumedAs && (
      <span className="muted small">
        {prefix} <RunLinkTo overview={overview} target={view.target} runId={view.resumedAs} now={now} />
      </span>
    );
  if (slice) {
    return (
      <div className="failure-actions">
        <a href={featureHref(slice.feature!, slice.slice)} className="button">
          Open slice {slice.slice} in {slice.feature} ›
        </a>
        {resumedAs('Zeroshot features resumed it as')}
      </div>
    );
  }
  if (observeOnly(view)) {
    // A configured source (such as pb's runs): the server refuses Resume/Replay for it, so none is offered.
    return (
      <div className="failure-actions">
        <span className="muted small">
          <Info size={12} aria-hidden /> {observeOnlyNote(view)}
        </span>
      </div>
    );
  }
  const { hasCheckpoints, waiting } = model;
  const run = (chosen: FailureAction) => {
    const action = effectiveAction(chosen, view.capabilities);
    if (action === 'resume') return onResume();
    onReplay(replayFocus(action, failure.node));
  };
  const icon = (action: FailureAction) => (action === 'resume' ? <RotateCcw size={14} aria-hidden /> : <Play size={14} aria-hidden />);
  const primary = failure.primary;
  const canEdit = (view.capabilities ?? []).includes('replay');
  // When the same settings would fail the same way, an action that cannot change them (resume,
  // or start again without replay-with-changes) sits behind the fix as a quiet text button.
  const doomed = unchangedFails(failure);
  const demoted = (a: FailureAction) => doomed && (a === 'resume' || !canEdit);
  const limit = primary === 'resume' ? resetNote(failure, now) : null;
  const note = actionsNote(view);
  return (
    <div className="failure-actions">
      {primary &&
        (primary === 'resume' && waiting ? (
          <button type="button" className="button primary" disabled title={`In ${untilText(failure.resetsAt!, now)}`}>
            <RotateCcw size={14} aria-hidden /> {resumeLabel(hasCheckpoints)} at {timeOfDay(failure.resetsAt!, now)}
          </button>
        ) : (
          <button type="button" className="button primary" onClick={() => run(primary)} title={failure.node && primary !== 'resume' ? replayVerbs(canEdit).hint : undefined}>
            {icon(primary)} {primaryLabel(effectiveAction(primary, view.capabilities), failure, hasCheckpoints, canEdit)}
          </button>
        ))}
      {limit && <span className="muted small">{waiting ? `(${limit})` : limit}</span>}
      {model.secondary.map((a) => (
        <button
          key={a}
          type="button"
          className={demoted(a) ? 'text-button demoted' : 'button'}
          onClick={() => run(a)}
          disabled={a === 'resume' && waiting}
          title={demoted(a) ? 'Runs again with the same settings, which failed this way' : undefined}
        >
          {icon(a)} {a === 'resume' ? resumeLabel(hasCheckpoints) : replayVerbs(canEdit).button}
        </button>
      ))}
      {resumedAs('Resumed as')}
      {note && (
        <span className="muted small" title={note === 'no checkpoints' ? 'This run saved no checkpoints, so it cannot start again from a node.' : undefined}>
          {note}
        </span>
      )}
    </div>
  );
}

/**
 * The failure card's one answer to "why": shown in full unless it is unusually long (a normal
 * 2-3 line paragraph is not clamped at all), in which case it expands in place rather than
 * sending the reader to the node panel for the full text.
 */
function ClampedMarkdown({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const long = text.split(/\s+/).filter(Boolean).length > 120;
  return (
    <div className={`clamped ${long && !open ? 'closed' : ''}`}>
      <Markdown text={text} />
      {long && (
        <button type="button" className="text-button" onClick={() => setOpen(!open)}>
          {open ? 'Show less' : 'Show all'}
        </button>
      )}
    </div>
  );
}
