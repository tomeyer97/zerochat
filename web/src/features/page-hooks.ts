/** The hooks the Features pages share: polling, the title, the toast, focus and Escape, stale data. */
import { useEffect, useRef, useState } from 'react';
import { errorMessage } from '../api';
import { useInterval, useNow, type Resource } from '../hooks';

/** The clock ticks every second while a feature is live. */
export const CLOCK_TICK_MS = 1000;
/** Reload every 3 s while something runs or a daemon is up… */
export const POLL_LIVE_MS = 3000;
/** …and every 15 s otherwise (a person may start it from the CLI). */
export const POLL_IDLE_MS = 15_000;

/** Reloads `resource` at the live or idle pace; returns the clock (ticking only while live). */
export function useFeaturePolling(resource: Pick<Resource<unknown>, 'reload'>, live: boolean): number {
  const now = useNow(live, CLOCK_TICK_MS);
  useInterval(resource.reload, live ? POLL_LIVE_MS : POLL_IDLE_MS, true);
  return now;
}

export function useDocumentTitle(title: string) {
  useEffect(() => {
    document.title = title;
  }, [title]);
}

export type Notice = { text: string; tone: 'info' | 'bad' };

/** How long a toast stays: errors longer, so there is time to read them. */
const TOAST_MS = { info: 5000, bad: 9000 } as const;

/** A confirmation that goes away on its own. */
export function useToast(): [Notice | null, (notice: Notice | null) => void] {
  const [notice, setNotice] = useState<Notice | null>(null);
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), TOAST_MS[notice.tone]);
    return () => window.clearTimeout(timer);
  }, [notice]);
  return [notice, setNotice];
}

/** Closing the slice panel returns focus to the card (graph node or list item) that opened it. */
export function useReturnFocus(slice: string | undefined) {
  const lastSlice = useRef<string | undefined>(slice);
  useEffect(() => {
    const previous = lastSlice.current;
    lastSlice.current = slice;
    if (previous && !slice) {
      const id = CSS.escape(previous);
      const card = document.querySelector<HTMLElement>(`.react-flow__node[data-id="${id}"], .slice-list-item[data-id="${id}"]`);
      card?.focus();
    }
  }, [slice]);
}

/** Escape calls `close` while `open`, unless a field has the keys or a dialog is open. */
export function useEscapeToClose(open: boolean, close: () => void) {
  const latestClose = useRef(close);
  latestClose.current = close;
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const el = event.target as HTMLElement | null;
      const typing = !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT');
      if (event.key === 'Escape' && open && !typing && !document.querySelector('dialog[open]')) {
        event.preventDefault();
        latestClose.current();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);
}

/**
 * One action at a time: `run(name, act)` marks `name` busy, clears the last error, and keeps
 * the error message if `act` throws (`onError` sees the cause too).
 */
export function useBusyAction<Name extends string>() {
  const [busy, setBusy] = useState<Name | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async (name: Name, act: () => Promise<void>, onError?: (cause: unknown) => void) => {
    setBusy(name);
    setError(null);
    try {
      await act();
    } catch (cause) {
      setError(errorMessage(cause));
      onError?.(cause);
    } finally {
      setBusy(null);
    }
  };
  return { busy, error, run };
}

/**
 * The last value that was not `undefined`. A resource whose key changes (a new state stamp, a
 * diff toggled on) has no data while the new key loads; showing the last answer meanwhile keeps
 * the section from flashing empty.
 */
export function useLastDefined<T>(value: T | undefined): T | undefined {
  const last = useRef(value);
  if (value !== undefined) last.current = value;
  return last.current;
}
