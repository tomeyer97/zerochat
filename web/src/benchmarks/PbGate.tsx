// Every Benchmarks page sits behind this gate: without pb (the ProgramBench runner) it shows one
// neutral page (no tabs, no Retry, no failing requests) instead of the page (pb-setup.ts). It asks
// the navigation and the setup check (`/api/setup`) first, so no benchmarks route is requested
// until both say pb is there.
import { useSyncExternalStore, type ComponentType, type ReactNode } from 'react';
import { FlaskConical } from 'lucide-react';
import { api as baseApi } from '../api';
import { useResource } from '../hooks';
import { fetchSetup } from '../onboarding/setup-api';
import { EmptyState, Skeleton } from '../common';
import { pbGateState, pbSaidNotConfigured, subscribePbSetup, type PbGateState } from './pb-setup';

export function PbNotConfigured() {
  return (
    <div className="home">
      <h1 className="visually-hidden">Benchmarks</h1>
      <EmptyState title="Benchmarks needs pb (ProgramBench runner), which is not configured here." icon={<FlaskConical size={22} aria-hidden />}>
        <p>
          Have pb? Set <code>pb_root</code> in zerochat&apos;s <code>config.toml</code> (<code>~/.config/zerochat/</code>), then reload.
        </p>
      </EmptyState>
    </div>
  );
}

export function PbGateView({ state, children }: { state: PbGateState; children?: ReactNode }) {
  if (state === 'missing') return <PbNotConfigured />;
  if (state === 'checking') {
    return (
      <div className="home">
        <Skeleton lines={6} />
      </div>
    );
  }
  return <>{children}</>;
}

export function PbGate({ children }: { children: ReactNode }) {
  const nav = useResource('benchmarks/pb-setup', (signal) => baseApi.overviewNav(signal));
  const setup = useResource('benchmarks/setup', (signal) => fetchSetup(signal));
  const said = useSyncExternalStore(subscribePbSetup, pbSaidNotConfigured);
  const setupPb = setup.data ? setup.data.pb : setup.error ? null : 'loading';
  return <PbGateView state={pbGateState(nav.data, !!nav.error, said, setupPb)}>{children}</PbGateView>;
}

/** `Page`, behind the gate. */
export function gated<P extends object>(Page: ComponentType<P>): ComponentType<P> {
  const Gated = (props: P) => (
    <PbGate>
      <Page {...props} />
    </PbGate>
  );
  Gated.displayName = `Gated(${Page.displayName ?? Page.name})`;
  return Gated;
}
