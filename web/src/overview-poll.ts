// The overview (every target with its runs, plus what the top nav needs), polled for the whole
// app: Home's table and the run page's sidebar show it, every page's nav reads it.
import { useEffect, useRef, useState } from 'react';
import { api, errorMessage } from './api';
import type { Overview } from './types';
import { mergeOverview, overviewDelay, type OverviewMode } from './status';
import { createPoller } from './poller';

export type OverviewState = {
  /** The last full answer (every target with its runs); `undefined` until one arrived. */
  data?: Overview;
  /** What the top nav needs (targets, the Features badge, the Zeroshot version): from whichever
   * answer, full or nav-only, came last. */
  nav?: Overview;
  error?: string;
  loading: boolean;
  reloading: boolean;
  reload: () => void;
};

/**
 * The overview, polled by `createPoller` (no overlapping polls; see there) at the pace of `mode`
 * (`overviewDelay`: every 4 s on a page with the run list, 15 s in a hidden tab).
 * - `slow`: an open run already has its own live event stream for its own row, so the list need
 *   not be fresh to the second; it still stays within a few seconds, so a replay just started or
 *   another run finishing show up promptly.
 * - `nav`: no run list on screen, so only the nav is fetched, rarely. Switching back to a page
 *   with the list polls the full list at once.
 * A run that cannot be read for a moment keeps its last good row (`mergeOverview`).
 */
export function useOverview(mode: OverviewMode): OverviewState {
  const [data, setData] = useState<Overview>();
  const [nav, setNav] = useState<Overview>();
  const [error, setError] = useState<string>();
  const [reloading, setReloading] = useState(false);
  const modeRef = useRef(mode);
  modeRef.current = mode;
  const [poller] = useState(() =>
    createPoller({
      poll: async () => {
        setReloading(true);
        const navOnly = modeRef.current === 'nav';
        try {
          const next = navOnly ? await api.overviewNav() : await api.overview();
          if (!navOnly) setData((prev) => mergeOverview(prev, next));
          setNav(next);
          setError(undefined);
        } catch (cause) {
          setError(errorMessage(cause));
        } finally {
          setReloading(false);
        }
      },
      delay: () => overviewDelay(modeRef.current, document.visibilityState === 'hidden'),
    })
  );
  useEffect(() => {
    poller.start();
    return () => poller.stop();
  }, [poller]);
  // Back on a page with the run list after a nav-only stretch: refresh it now, not in 30 s.
  const lastMode = useRef(mode);
  useEffect(() => {
    const was = lastMode.current;
    lastMode.current = mode;
    if (was === 'nav' && mode !== 'nav') poller.poke();
  }, [mode, poller]);
  return { data, nav: nav ?? data, error, loading: !data && !error, reloading, reload: poller.poke };
}
