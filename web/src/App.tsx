// The app shell: the header with its section nav, the run sidebar (on a run page) and the page
// for the current route.
import { useEffect, useState, type ReactNode } from 'react';
import { GitBranch, Menu, MessageSquare, PanelLeftClose, PanelLeftOpen, X } from 'lucide-react';
import { benchmarksHref, featuresHref, pageTitle, parseRoute, type Route } from './route';
import { BenchmarkBatchPage, BenchmarksCompare, BenchmarksList, BenchmarksStart, BenchmarksSweepReport, BenchmarksSweeps } from './Benchmarks';
import { FeaturePage, FeaturesList } from './Features';
import { useDocumentTitle, useNow, useStoredState, useTitleBadge } from './hooks';
import { isLive, listingStatus, type OverviewMode } from './status';
import { useOverview, type OverviewState } from './overview-poll';
import { Home } from './Home';
import { Sidebar } from './Sidebar';
import { RunPage } from './RunPage';
import { ThemeControl } from './components/ThemeControl';
import { cx, EmptyState, ErrorBoundary } from './common';
import type { Setup } from './generated/discovery';
import { useSetup } from './onboarding/setup-api';
import { brandTitle, setupLine } from './onboarding/setup-model';
import { GlossaryButton } from './onboarding/Glossary';
import { ZeroshotStrip } from './onboarding/ZeroshotStrip';

// The overview's state type moved with its hook; re-exported for existing imports.
export type { OverviewState } from './overview-poll';

function useRoute(): Route {
  const [route, setRoute] = useState(() => parseRoute(location.hash));
  useEffect(() => {
    const onChange = () => setRoute(parseRoute(location.hash));
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  return route;
}

/** How often the overview is polled on this page (see `useOverview`). */
function overviewMode(route: Route, runLive: boolean): OverviewMode {
  if (route.kind === 'home') return 'full';
  if (route.kind === 'run') return runLive ? 'slow' : 'full';
  return 'nav';
}

export function App() {
  const route = useRoute();
  const [runLive, setRunLive] = useState(false);
  const overview = useOverview(overviewMode(route, runLive));
  const setup = useSetup();
  // Home with no runs shows the first-run card, which already carries the Zeroshot check (and
  // until Home's list arrives it is not known whether it has runs: no strip flashing first).
  const firstRun = route.kind === 'home' && (!overview.data || overview.data.targets.every((t) => t.runs.length === 0));
  // One clock for every elapsed time on screen: it ticks each second while a run is live.
  const anyLive = !!overview.data?.targets.some((t) => t.runs.some((r) => isLive(listingStatus(r))));
  const now = useNow(true, anyLive ? 1000 : 30_000);
  const [drawer, setDrawer] = useState(false);
  const [listHidden, setListHidden] = useStoredState('zerochat.runListHidden', false);
  const narrow = () => window.matchMedia('(max-width: 900px)').matches;
  const selected = route.kind === 'run' ? { target: route.target, run: route.run } : null;
  // pb (the ProgramBench runner) feeds zerochat as a configured target with the id `pb`; the
  // Benchmarks section reads pb's results, so it is shown only when that target is configured.
  const pbConfigured = !!overview.nav?.targets.some((t) => t.id === 'pb');
  const featuresConfigured = !!overview.nav?.features;
  const featuresNeedYou = overview.nav?.featuresNeedYou ?? 0;
  useTitleBadge(featuresNeedYou);
  useDocumentTitle(pageTitle(route));
  useEffect(() => setDrawer(false), [route]);
  const onBenchmarks = route.kind.startsWith('benchmarks');
  const onFeatures = route.kind === 'features' || route.kind === 'feature';
  const onRuns = !onBenchmarks && !onFeatures;
  const layout = cx('app', route.kind === 'run' ? 'on-run' : 'on-home', drawer && 'drawer-open', listHidden && 'list-hidden');
  return (
    <div className={layout}>
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <header className="app-header">
        {route.kind === 'run' && (
          <>
            <button type="button" className="icon-button drawer-toggle" aria-label={drawer ? 'Hide runs' : 'Show runs'} aria-expanded={drawer} onClick={() => setDrawer(!drawer)}>
              {drawer ? <X size={18} /> : <Menu size={18} />}
            </button>
            <button
              type="button"
              className="icon-button list-toggle"
              aria-label={listHidden ? 'Show runs' : 'Hide runs'}
              aria-expanded={!listHidden}
              title={listHidden ? 'Show runs' : 'Hide runs'}
              onClick={() => (narrow() ? setDrawer(!drawer) : setListHidden(!listHidden))}
            >
              {listHidden ? <PanelLeftOpen size={18} /> : <PanelLeftClose size={18} />}
            </button>
          </>
        )}
        <a href="#/" className="brand" aria-label="zerochat home" title={brandTitle(setup, overview.nav?.zeroshot.version)}>
          <MessageSquare size={20} aria-hidden strokeWidth={1.9} />
          <span>zerochat</span>
        </a>
        {(pbConfigured || featuresConfigured) && (
          <nav className="top-nav" aria-label="Sections">
            <a href="#/" aria-current={onRuns ? 'page' : undefined} className={cx(onRuns && 'on')}>
              Runs
            </a>
            {featuresConfigured && (
              <>
                <span aria-hidden>·</span>
                <a href={featuresHref()} aria-current={onFeatures ? 'page' : undefined} className={cx(onFeatures && 'on')}>
                  Features
                  {featuresNeedYou > 0 && (
                    <span className="nav-badge" title={`${featuresNeedYou} need${featuresNeedYou === 1 ? 's' : ''} you`}>
                      <span className="visually-hidden">, </span>
                      {featuresNeedYou}
                      <span className="visually-hidden"> need you</span>
                    </span>
                  )}
                </a>
              </>
            )}
            {pbConfigured && (
              <>
                <span aria-hidden>·</span>
                <a href={benchmarksHref()} aria-current={onBenchmarks ? 'page' : undefined} className={cx(onBenchmarks && 'on')}>
                  Benchmarks
                </a>
              </>
            )}
          </nav>
        )}
        <span className="header-spacer" />
        <SetupLine setup={setup} />
        <GlossaryButton setup={setup} />
        <ThemeControl />
      </header>
      {!firstRun && <ZeroshotStrip setup={setup} />}
      <div className="app-body">
        {route.kind === 'run' && <Sidebar overview={overview} selected={selected} now={now} />}
        {drawer && <button type="button" className="drawer-backdrop" aria-label="Hide runs" onClick={() => setDrawer(false)} />}
        <main id="main" className="main" tabIndex={-1}>
          {renderPage(route, { overview, now, setup, onRunLiveChange: setRunLive })}
        </main>
      </div>
    </div>
  );
}

/** Which Zeroshot zerochat uses and which folder it reads runs from, quiet in the top bar of every page. */
function SetupLine({ setup }: { setup: Setup | null }) {
  const line = setupLine(setup);
  if (!line) return null;
  return (
    <span className="setup-line" title={line.title}>
      {line.text}
    </span>
  );
}

/**
 * The page for `route`. Each page sits in its own ErrorBoundary, so a crash leaves the header (and
 * on a run page the sidebar) alone; the boundary's `key` remounts it when the route names
 * another item, so picking another run or batch recovers without a full reload.
 */
function renderPage(route: Route, { overview, now, setup, onRunLiveChange }: { overview: OverviewState; now: number; setup: Setup | null; onRunLiveChange: (live: boolean) => void }): ReactNode {
  switch (route.kind) {
    case 'home':
      return <Home overview={overview} now={now} setup={setup} />;
    case 'unknown':
      return (
        <EmptyState title="Page not found" icon={<GitBranch size={22} aria-hidden />}>
          <p>
            There is nothing at <code>#/{route.path}</code>.
          </p>
          <p>
            <a href="#/">All runs</a>
          </p>
        </EmptyState>
      );
    case 'run':
      return (
        <ErrorBoundary key={`${route.target}/${route.run}`}>
          <RunPage route={route} overview={overview.data} onLiveChange={onRunLiveChange} />
        </ErrorBoundary>
      );
    case 'features':
      return (
        <ErrorBoundary>
          <FeaturesList />
        </ErrorBoundary>
      );
    case 'feature':
      return (
        <ErrorBoundary key={route.name}>
          <FeaturePage name={route.name} slice={route.slice} />
        </ErrorBoundary>
      );
    case 'benchmarks':
      return (
        <ErrorBoundary>
          <BenchmarksList />
        </ErrorBoundary>
      );
    case 'benchmarksBatch':
      return (
        <ErrorBoundary key={route.id}>
          <BenchmarkBatchPage id={route.id} />
        </ErrorBoundary>
      );
    case 'benchmarksCompare':
      return (
        <ErrorBoundary key={`${route.a ?? ''}/${route.b ?? ''}`}>
          <BenchmarksCompare a={route.a} b={route.b} />
        </ErrorBoundary>
      );
    case 'benchmarksStart':
      return (
        <ErrorBoundary>
          <BenchmarksStart />
        </ErrorBoundary>
      );
    case 'benchmarksSweeps':
      return (
        <ErrorBoundary>
          <BenchmarksSweeps />
        </ErrorBoundary>
      );
    case 'benchmarksSweepReport':
      return (
        <ErrorBoundary key={route.ids.join(',')}>
          <BenchmarksSweepReport route={route} />
        </ErrorBoundary>
      );
  }
}
