// One run: its header, graph, timeline and logs, with a node or files panel beside them and the
// run's dialogs.
import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { GitBranch, Info, X } from 'lucide-react';
import { api, retryable, runPath } from './api';
import type { Overview, RunView } from './types';
import { isLive, isWatchable, runStatus, stepExecution } from './status';
import { documentTitle, navigate, resolveVisit, visitFallback, visitOf, type Route } from './route';
import { useDocumentTitle, useInterval, useNow, useResource, useRunEvents, useStoredState, type Resource } from './hooks';
import { cx, EmptyState, ErrorBox, Skeleton } from './common';
import { RunHeader } from './RunHeader';
import { FailureCard } from './FailureCard';
import { PressureStrip } from './PressureStrip';
import { RunLogs } from './RunLogs';
import type { ReplayFocus } from './run-model';
import { ScoreCard } from './Benchmarks';
import { Graph } from './Graph';
import { Timeline } from './Timeline';
import { NodePanel } from './NodePanel';
import { RunFilesPanel } from './Files';
import { RunDialogs, type RunDialog } from './RunDialogs';
import { useRunShortcuts } from './run-shortcuts';
import { PanelResizer } from './components/PanelResizer';

type RunRoute = Extract<Route, { kind: 'run' }>;
type Notice = { text: string };

export function RunPage({ route, overview, onLiveChange }: { route: RunRoute; overview?: Overview; onLiveChange?: (live: boolean) => void }) {
  const { target, run } = route;
  const resource = useResource<RunView>(`${target}/${run}`, (signal) => api.run(target, run, signal));
  const [historyError, setHistoryError] = useState<string | null>(null);
  const pageRef = useRef<HTMLDivElement | null>(null);
  const [panelWidth, setPanelWidth] = useStoredState<number | null>('zerochat.panelWidth', null);
  const view = resource.data;
  const status = view ? runStatus(view) : 'unknown';
  // Subscribe only while the run can still change (a missing run would otherwise retry its
  // event stream forever; a finished run never gets a `changed` event again). A stalled run's
  // controller is gone, but Zeroshot hasn't closed it yet, so it still needs updates.
  const live = useRunEvents(view && isWatchable(status) ? `${runPath(target, run)}/events` : null, { changed: resource.reload, historyError: setHistoryError });
  const now = useNow(!!view && isWatchable(status));
  const visit = resolveVisit(view ?? { executions: [] }, route);
  useEffect(() => {
    onLiveChange?.(live === 'live');
    return () => onLiveChange?.(false);
  }, [live, onLiveChange]);
  // While the stream is down, poll like the sidebar does so the view stays fresh instead of
  // freezing (the ticking duration would otherwise look live while it is not) -- including a
  // stalled run, which otherwise never notices Zeroshot finally closing it.
  useInterval(resource.reload, 4_000, isWatchable(status) && live !== 'live');
  const [dialog, setDialog] = useState<RunDialog | null>(null);
  const [replayFocus, setReplayFocus] = useState<ReplayFocus>(null);
  const missingRun = resource.errorStatus === 404;
  // Every navigation names the tab: the run (and node) once loaded, plain while loading.
  useDocumentTitle(view ? documentTitle(route.node, view.title || 'Untitled run') : documentTitle(missingRun ? 'Run not found' : null));
  const [notice, setNotice] = useState<Notice | null>(null);
  const [stopRequested, setStopRequested] = useState(false);
  const panelOpen = !!route.node || !!route.files;

  const go = useCallback((next: Partial<RunRoute>, replace = false) => navigate({ kind: 'run', target, run, ...next }, replace), [target, run]);
  const close = useCallback(() => go({}), [go]);

  // A visit the node never had (e.g. a stale link): correct the address to its latest visit,
  // with a brief, dismissible notice rather than showing something else without a word.
  useEffect(() => {
    const stale = view ? visitFallback(view, route) : null;
    if (!stale) return;
    go({ node: route.node, ...visitOf(stale.fallback) }, true);
    setNotice({ text: stale.notice });
  }, [view, route, go]);

  useRunShortcuts({
    enabled: !dialog,
    panelOpen,
    onClose: close,
    onStep: (delta) => {
      const next = view && stepExecution(view, resolveVisit(view, route).execution?.id, delta);
      if (next) go({ node: next.node, ...visitOf(next) });
      return !!next;
    },
  });

  if (!view) return <RunPageEmpty resource={resource} missingRun={missingRun} />;

  const openFiles = () => go({ files: true });
  return (
    <div className={cx('run-page', panelOpen && 'with-panel')} ref={pageRef} style={panelWidth ? ({ '--panel-w': `${panelWidth}px` } as CSSProperties) : undefined}>
      <div className="run-scroll" id="run-content">
        <RunHeader
          view={view}
          now={now}
          live={live}
          stale={historyError ?? resource.error ?? null}
          stopping={stopRequested && isLive(status)}
          overview={overview}
          onStop={() => setDialog('stop')}
          onReplay={() => {
            setReplayFocus(null);
            setDialog('replay');
          }}
          onFiles={openFiles}
        />
        <ScoreCard view={view} />
        {notice && (
          <div className="toast tone-info" role="status">
            <Info size={15} aria-hidden />
            <span>{notice.text}</span>
            <button type="button" className="icon-button tiny" aria-label="Dismiss" onClick={() => setNotice(null)}>
              <X size={13} />
            </button>
          </div>
        )}
        {view.failure && (
          <FailureCard
            failure={view.failure}
            view={view}
            now={Date.now()}
            overview={overview}
            onReplay={(focus) => {
              setReplayFocus(focus);
              setDialog('replay');
            }}
            onResume={() => setDialog('resume')}
            panelOpen={panelOpen}
          />
        )}
        <PressureStrip pressures={view.pressures} view={view} now={now} />
        <section className="graph-section" aria-label="Graph">
          <Graph view={view} selectedNode={route.node} onOpen={(node) => go({ node })} />
        </section>
        <Timeline view={view} selectedExecution={visit.execution?.id} />
        <RunLogs view={view} />
      </div>
      {panelOpen && <PanelResizer pageRef={pageRef} width={panelWidth} onWidth={setPanelWidth} />}
      {route.node && <NodePanel key={route.node} view={view} node={route.node} execution={visit.execution?.id} onClose={close} onSelect={(e) => go({ node: route.node, ...visitOf(e) })} />}
      {route.files && !route.node && <RunFilesPanel view={view} onClose={close} />}
      <RunDialogs
        dialog={dialog}
        target={target}
        run={run}
        view={view}
        replayFocus={replayFocus}
        onClose={() => setDialog(null)}
        reload={resource.reload}
        onNotice={(text) => setNotice({ text })}
        onStopRequested={() => setStopRequested(true)}
      />
    </div>
  );
}

/** The page before a run view arrived: a skeleton while loading, else why it is missing. */
function RunPageEmpty({ resource, missingRun }: { resource: Resource<RunView>; missingRun: boolean }) {
  if (resource.loading) {
    return (
      <div className="run-page">
        <div className="run-scroll">
          <Skeleton lines={2} />
          <Skeleton height={340} />
          <Skeleton lines={6} />
        </div>
      </div>
    );
  }
  // A retry only helps when the failure may clear up: a transport failure or a server error.
  const canRetry = retryable(resource.errorStatus) || (resource.errorStatus ?? 0) >= 500;
  return (
    <div className="run-page">
      <div className="run-scroll">
        {missingRun ? (
          <EmptyState title="Run not found" icon={<GitBranch size={22} aria-hidden />}>
            <p>
              <a href="#/">All runs</a>
            </p>
          </EmptyState>
        ) : (
          <>
            <ErrorBox message={resource.error ?? 'Could not load this run.'} onRetry={canRetry ? resource.reload : undefined} />
            <p className="muted">
              <a href="#/">All runs</a>
            </p>
          </>
        )}
      </div>
    </div>
  );
}
