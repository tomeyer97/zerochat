import { FileDiff, Info, Play, Square } from 'lucide-react';
import type { Overview, RunView } from './types';
import { missingDataWhy, observeOnlyNote } from './status';
import { LIST_PRICE_NOTE, costText, duration, fullTime, lanesLine, shortId, timeOfDay } from './format';
import { benchmarksBatchHref, featureHref, runHref } from './route';
import { CopyButton, StatusPill } from './common';
import type { LiveState } from './hooks';
import { costTitle, runElapsed, runHeaderModel, runLink } from './run-model';
import { replayVerbs } from './replay-model';

/** A link to another run, named by when it started. */
export function RunLinkTo({ overview, target, runId, now }: { overview: Overview | undefined; target: string; runId: string; now: number }) {
  const { label, title } = runLink(overview, target, runId, now);
  return (
    <a href={runHref(target, runId)} title={title}>
      {label}
    </a>
  );
}

type HeaderModel = ReturnType<typeof runHeaderModel>;

export function RunHeader({
  view,
  now,
  live,
  stale,
  stopping,
  overview,
  onStop,
  onReplay,
  onFiles,
}: {
  view: RunView;
  now: number;
  live: LiveState;
  /** Why the page stopped updating (a failed refresh or an unreadable ledger), else null. */
  stale: string | null;
  /** A force stop was requested and the run has not stopped yet. */
  stopping: boolean;
  overview: Overview | undefined;
  onStop: () => void;
  onReplay: () => void;
  onFiles: () => void;
}) {
  const model = runHeaderModel(view, stopping);
  return (
    <header className="run-header">
      <div className="run-title-row">
        <h1 className="run-title-text" title={view.title}>
          {view.title || 'Untitled run'}
        </h1>
        <StatusPill status={model.shownStatus} />
        <div className="run-actions">
          {model.observed && model.live && (
            <span className="muted small observe-note" title={observeOnlyNote(view)}>
              <Info size={12} aria-hidden /> observed only
            </span>
          )}
          {model.status === 'running' && !stopping && !model.observed && (
            <button type="button" className="button" onClick={onStop} title="Stop every agent of this run now">
              <Square size={14} aria-hidden /> Force stop
            </button>
          )}
          {model.replay && (
            <button type="button" className="button" onClick={onReplay} title={replayVerbs(true).hint}>
              <Play size={14} aria-hidden /> {replayVerbs(true).button}
            </button>
          )}
          {model.files && (
            <button type="button" className="button" onClick={onFiles} title="Files the run changed, with diffs">
              <FileDiff size={14} aria-hidden /> Files
            </button>
          )}
        </div>
      </div>
      <RunMetaLine view={view} model={model} now={now} live={live} stale={stale} />
      {view.context && <ContextLine context={view.context} />}
      {model.hasLineage && <Lineage view={view} replayOf={model.replayOf} overview={overview} now={now} />}
    </header>
  );
}

/** What the background figure is, as its tooltip says. */
export const BACKGROUND_NOTE = 'Tokens of background agents (Active Memory); Zeroshot does not price them.';

/** The cost's tooltip: an estimate says once what `~$` means (`LIST_PRICE_NOTE`). */
export function runCostTitle(view: Parameters<typeof costTitle>[0]): string {
  return view.costEstimated && view.costSource !== 'pb' ? LIST_PRICE_NOTE : costTitle(view);
}

/** Started, duration, cost, tokens, lanes, connection state, limited data and the run id, in one line. */
function RunMetaLine({ view, model, now, live, stale }: { view: RunView; model: HeaderModel; now: number; live: LiveState; stale: string | null }) {
  const { tokens, background, missing } = model;
  const lanes = lanesLine(view.lanes);
  return (
    <div className="run-meta-line">
      {/* Redundant with the sidebar's own row on a phone, where width is tightest. */}
      <span className="meta-started" title={view.startedAt ? `Started ${fullTime(view.startedAt, now)}` : ''}>
        {timeOfDay(view.startedAt, now)}
      </span>
      <span title={view.endedAt ? `Ended ${fullTime(view.endedAt, now)}` : ''}>{duration(runElapsed(view, now))}</span>
      {model.showCost && <span title={runCostTitle(view)}>{costText(view.costUsd, view.costEstimated)}</span>}
      {!model.noTokens ? (
        <span title={model.usageTitle}>
          {tokens.total}
          {tokens.cached && <span className="muted meta-cached"> ({tokens.cached})</span>}
          {background && (
            <span className="muted meta-background" title={BACKGROUND_NOTE}>
              {' '}
              · +{background} background
            </span>
          )}
        </span>
      ) : view.usagePending ? (
        <span className="muted" title="Zeroshot counts tokens when a node's turn ends.">
          — tokens
        </span>
      ) : (
        !model.showCost && <span className="muted">no usage recorded</span>
      )}
      {lanes && (
        <span className="meta-lane" title={lanes.title}>
          {lanes.text}
        </span>
      )}
      {stale ? (
        <span className="stale" title={stale}>
          not updating
        </span>
      ) : (
        model.live &&
        live === 'reconnecting' && (
          <span className="stale" title="Not connected to the run's event stream; retrying.">
            reconnecting
          </span>
        )
      )}
      {missing.length > 0 && (
        <details className="limited">
          <summary>
            <Info size={12} aria-hidden /> limited data
          </summary>
          <div className="popover">
            <p>Not shown for this run:</p>
            <ul>
              {missingDataWhy(view).map((m) => (
                <li key={m.what}>
                  {m.what}
                  <span className="muted">: {m.why}</span>
                </li>
              ))}
            </ul>
          </div>
        </details>
      )}
      <CopyButton text={view.runId} label="Copy run id" preview={shortId(view.runId, 12)} />
    </div>
  );
}

/** The runs this one came from and led to: replay of, resumed from/as, replayed as. */
function Lineage({ view, replayOf, overview, now }: { view: RunView; replayOf: string | undefined; overview: Overview | undefined; now: number }) {
  const link = (id: string) => <RunLinkTo key={id} overview={overview} target={view.target} runId={id} now={now} />;
  // `replayOf`/`replays` are written only by a Zeroshot that replays with changes; `resumedFrom`/
  // `resumedAs` by any resume, including a start again from a checkpoint.
  const edited = replayVerbs(true);
  const unchanged = replayVerbs(false);
  return (
    <p className="lineage">
      {replayOf && (
        <span title={view.replayOf?.node ? `From the checkpoint before ${view.replayOf.node}` : undefined}>
          {edited.lineageFrom} {link(replayOf)}
        </span>
      )}
      {view.resumedFrom && (
        <span>
          {unchanged.lineageFrom} {link(view.resumedFrom)}
        </span>
      )}
      {view.resumedAs && (
        <span>
          {unchanged.lineageAs} {link(view.resumedAs)}
        </span>
      )}
      {view.replays.length > 0 && (
        <span>
          {edited.lineageAs} {view.replays.map((id, i) => [i > 0 ? ', ' : '', link(id)])}
        </span>
      )}
    </p>
  );
}

/** The run's place in a feature or a benchmark batch, linking up to it. */
function ContextLine({ context }: { context: NonNullable<RunView['context']> }) {
  const href = context.kind === 'slice' && context.feature ? featureHref(context.feature, context.slice) : context.kind === 'benchmark' && context.parent ? benchmarksBatchHref(context.parent) : null;
  return (
    <p className="run-context-line muted">
      {href ? (
        <a href={href} title={context.label}>
          {context.line} ›
        </a>
      ) : (
        context.line
      )}
    </p>
  );
}
