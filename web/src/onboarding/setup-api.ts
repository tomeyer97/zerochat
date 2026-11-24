// The setup check (`GET /api/setup`, the server's `discovery::Setup`) for the first-run card, the
// Zeroshot strip and the logo's tooltip.
import { useEffect, useState } from 'react';
import { request } from '../api';
import type { Setup } from '../generated/discovery';

export function fetchSetup(signal?: AbortSignal): Promise<Setup> {
  return request<Setup>('/api/setup', 'check the setup', { signal });
}

/**
 * The setup check, read once and again whenever the tab comes back into view (someone who just
 * installed Zeroshot in a terminal sees the strip go). `null` until it answers, or when it fails:
 * the pages then show what the overview alone says.
 */
export function useSetup(): Setup | null {
  const [setup, setSetup] = useState<Setup | null>(null);
  useEffect(() => {
    let controller: AbortController | null = null;
    const load = () => {
      controller?.abort();
      const mine = new AbortController();
      controller = mine;
      fetchSetup(mine.signal).then(
        (next) => {
          if (!mine.signal.aborted) setSetup(next);
        },
        () => undefined
      );
    };
    const onVisible = () => {
      if (document.visibilityState === 'visible') load();
    };
    load();
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      controller?.abort();
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, []);
  return setup;
}
