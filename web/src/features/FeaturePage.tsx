import { useRef, type CSSProperties } from 'react';
import { ChevronRight, Layers, X } from 'lucide-react';
import { api } from '../api';
import type { FeatureDetail } from '../types';
import { findPaddedSlice, isLiveRow, toastText } from '../features-model';
import { featuresHref, navigate } from '../route';
import { useResource, useStoredState } from '../hooks';
import { EmptyState, ErrorBox, Skeleton } from '../common';
import { PanelResizer } from '../components/PanelResizer';
import { FeatureChangesSection } from './FeatureChanges';
import { FeatureHeader, type OnDone } from './FeatureHeader';
import { FeatureTimeline } from './FeatureTimeline';
import { SliceGraph } from './SliceGraph';
import { SlicePanel } from './SlicePanel';
import { useDocumentTitle, useEscapeToClose, useFeaturePolling, useReturnFocus, useToast, type Notice } from './page-hooks';

/** A feature: header, slice graph, changes, timeline, and the slice panel. `#/features/<name>[/slice/<id>]`. */
export function FeaturePage({ name, slice }: { name: string; slice?: string }) {
  const resource = useResource<FeatureDetail>(`feature/${name}`, (signal) => api.feature(name, signal));
  const detail = resource.data;
  const now = useFeaturePolling(resource, !!detail && isLiveRow(detail));
  const pageRef = useRef<HTMLDivElement | null>(null);
  const [panelWidth, setPanelWidth] = useStoredState<number | null>('zerochat.panelWidth', null);
  const [notice, setNotice] = useToast();
  const open = (id?: string) => navigate({ kind: 'feature', name, slice: id });
  const known = !!detail && (!slice || detail.slices.some((s) => s.id === slice));
  const notFound = resource.errorStatus === 404 || (!!detail && !!slice && !known);
  useDocumentTitle(notFound ? 'Not found · zerochat' : `${slice ? `${slice} · ` : ''}${name} · Features · zerochat`);
  useReturnFocus(slice);
  useEscapeToClose(!!slice, () => open());

  if (!detail) return <FeaturePageFallback name={name} loading={resource.loading} notFound={resource.errorStatus === 404} error={resource.error} onRetry={resource.reload} />;

  const selected = slice ? detail.slices.find((s) => s.id === slice) : undefined;
  const padded = slice && !selected ? findPaddedSlice(detail.slices, slice) : undefined;
  if (padded) {
    // Replace `/slice/4` with `/slice/04` once this render is over: `navigate` fires `hashchange`
    // at once, and the app's listener sets its route state, which React forbids during a render.
    window.setTimeout(() => navigate({ kind: 'feature', name, slice: padded.id }, true), 0);
  }
  const done: OnDone = (message, tone = 'info') => {
    if (message) setNotice({ text: toastText(message, detail.name), tone });
    resource.reload();
  };
  return (
    <div className={`run-page ${selected ? 'with-panel' : ''}`} ref={pageRef} style={panelWidth ? ({ '--panel-w': `${panelWidth}px` } as CSSProperties) : undefined}>
      <div className="run-scroll">
        <FeatureHeader detail={detail} now={now} onDone={done} onOpen={open} stale={resource.error ?? null} />
        {slice && !selected && (
          <p className="feature-missing" role="alert">
            {detail.name} has no slice {slice}.{' '}
            <button type="button" className="text-button" onClick={() => open()}>
              Show all slices
            </button>
          </p>
        )}
        <section className="graph-section" aria-label="Slices">
          <SliceGraph slices={detail.slices} selected={slice} now={now} daemonUp={detail.daemonPid != null} pollSecs={detail.pollSecs} onOpen={(id) => open(id)} />
        </section>
        <FeatureChangesSection detail={detail} />
        <FeatureTimeline name={name} stamp={`${detail.updatedAt}-${detail.slices.map((s) => s.attempts.length).join('')}`} slices={detail.slices.map((s) => s.id)} selected={slice} />
      </div>
      {selected && <PanelResizer pageRef={pageRef} width={panelWidth} onWidth={setPanelWidth} />}
      {selected && <SlicePanel key={selected.id} feature={detail} slice={selected} now={now} onClose={() => open()} onOpen={open} onDone={done} onRefused={() => resource.reload()} />}
      {notice && <FeatureToast notice={notice} onDismiss={() => setNotice(null)} />}
    </div>
  );
}

/** The page before the feature loads: a skeleton, "no such feature", or the error with a retry. */
function FeaturePageFallback({ name, loading, notFound, error, onRetry }: { name: string; loading: boolean; notFound: boolean; error?: string; onRetry: () => void }) {
  return (
    <div className="run-page">
      <div className="run-scroll">
        <p className="muted small crumbs">
          <a href={featuresHref()}>Features</a> <ChevronRight size={12} aria-hidden /> {name}
        </p>
        {loading ? (
          <>
            <Skeleton lines={2} />
            <Skeleton height={260} />
          </>
        ) : notFound ? (
          <EmptyState title={`No feature called “${name}”`} icon={<Layers size={22} aria-hidden />}>
            <p>
              <a href={featuresHref()}>All features</a>
            </p>
          </EmptyState>
        ) : (
          <>
            <h1 className="run-title-text">{name}</h1>
            <ErrorBox message={error ?? 'Could not load this feature.'} onRetry={onRetry} />
          </>
        )}
      </div>
    </div>
  );
}

/**
 * An action's confirmation, floating bottom-right so it never moves the page. Unlike the
 * shared `.toast` (an inline banner on the run page) it leaves on its own (`useToast`).
 */
function FeatureToast({ notice, onDismiss }: { notice: Notice; onDismiss: () => void }) {
  return (
    <div className={`feature-toast tone-${notice.tone}`} role="status">
      <span>{notice.text}</span>
      <button type="button" className="icon-button tiny" aria-label="Dismiss" onClick={onDismiss}>
        <X size={13} />
      </button>
    </div>
  );
}
