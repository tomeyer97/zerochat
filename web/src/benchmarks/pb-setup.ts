// Whether pb (the ProgramBench runner) is configured here. Without it every `/api/benchmarks/*`
// route answers 404 `pb_not_configured`: not a failure, the Benchmarks pages show one neutral
// page instead (PbGate.tsx). The pages ask the navigation first (the `pb` target exists exactly
// when `pb_root` is set), so an unconfigured zerochat never requests a benchmarks route at all.
import { ApiError, api as baseApi } from '../api';
import type { Overview } from '../types';

export const PB_NOT_CONFIGURED = 'pb_not_configured';

/** The server's "pb is not configured" answer (any benchmarks route, GET or POST). */
export function isPbNotConfigured(cause: unknown): boolean {
  return cause instanceof ApiError && cause.code === PB_NOT_CONFIGURED;
}

export type PbGateState = 'checking' | 'missing' | 'ready';

/** The setup check's answer about pb: `'loading'` until it answers, `null` when it failed. */
export type SetupPb = { configured: boolean } | 'loading' | null;

/** What a Benchmarks page shows: the neutral page when the setup check says pb is not configured,
 * the navigation has no `pb` target, or a benchmarks route said `pb_not_configured`; the page
 * itself once both the navigation and the setup check have answered and neither says no (one that
 * could not be read does not hold it up: the page's own request then says what is wrong). */
export function pbGateState(nav: Pick<Overview, 'targets'> | undefined, navFailed: boolean, saidNotConfigured: boolean, setupPb: SetupPb = null): PbGateState {
  if (saidNotConfigured || (setupPb && setupPb !== 'loading' && !setupPb.configured)) return 'missing';
  if (nav && !nav.targets.some((t) => t.id === 'pb')) return 'missing';
  if (setupPb === 'loading') return 'checking';
  if (nav) return 'ready';
  return navFailed ? 'ready' : 'checking';
}

// ---- "a benchmarks route said pb_not_configured": one flag until the page is reloaded ----

let notConfigured = false;
const listeners = new Set<() => void>();

export function pbSaidNotConfigured(): boolean {
  return notConfigured;
}

export function subscribePbSetup(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Notes a `pb_not_configured` answer; any other cause is left alone. */
export function notePbAnswer(cause: unknown): void {
  if (!isPbNotConfigured(cause) || notConfigured) return;
  notConfigured = true;
  for (const l of listeners) l();
}

/** `api`, with every failure checked for `pb_not_configured` (the Benchmarks pages use this). */
export const api: typeof baseApi = Object.fromEntries(
  Object.entries(baseApi).map(([name, call]) => [
    name,
    (...args: unknown[]) =>
      (call as (...a: unknown[]) => Promise<unknown>)(...args).catch((cause: unknown) => {
        notePbAnswer(cause);
        throw cause;
      }),
  ])
) as typeof baseApi;
