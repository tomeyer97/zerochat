import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError, errorMessage } from './api';
import { safeStorage } from './browser';

/** The current time, re-rendered every `interval` ms while `enabled`. */
export function useNow(enabled = true, interval = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!enabled) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), interval);
    return () => window.clearInterval(id);
  }, [enabled, interval]);
  return now;
}

export type Resource<T> = {
  data: T | undefined;
  error: string | undefined;
  /** The HTTP status of the last error (4xx: retrying the same request cannot help). */
  errorStatus: number | undefined;
  /** True only until the first answer (reloads keep showing the previous data). */
  loading: boolean;
  reloading: boolean;
  reload: () => void;
};

/** What `useResource` remembers: the resource as last loaded for `key`, without `reload`. */
export type ResourceState<T> = { key: string | null } & Pick<Resource<T>, 'loading' | 'reloading'> & Partial<Pick<Resource<T>, 'data' | 'error' | 'errorStatus'>>;

/**
 * Whether `useResource` retries a failed load by itself: a network failure or a 5xx may clear up
 * on its own; a 4xx will answer the same again. (A panel's own Retry button can follow a stricter
 * rule for its endpoint, as the Files panel does.)
 */
export function autoRetryable(status: number | undefined): boolean {
  return status == null || status === 0 || status >= 500;
}

/**
 * The state a load of `key` starts from: the same key reloads in the background and keeps its
 * data (`reloading`); a new key starts over (`loading`, no data).
 */
export function startLoad<T>(state: ResourceState<T>, key: string): ResourceState<T> {
  return state.key === key ? { ...state, reloading: true } : { key, loading: true, reloading: false };
}

/**
 * Loads `key` with `load`; keeps the last good data while reloading so nothing flashes, and
 * reports errors as the server's own message. A new `key` starts over. A new `refresh` value
 * reloads the same key in the background (the key names what is shown; `refresh` says it may
 * have changed, e.g. a live run's event cursor). A transient failure (network, 5xx) retries
 * itself with backoff, in addition to `reload`.
 */
export function useResource<T>(key: string | null, load: (signal: AbortSignal) => Promise<T>, refresh?: string | number | null): Resource<T> {
  const [state, setState] = useState<ResourceState<T>>({ key, loading: !!key, reloading: false });
  const [tick, setTick] = useState(0);
  const loader = useRef(load);
  loader.current = load;
  const attempt = useRef(0);
  useEffect(() => {
    if (!key) {
      setState({ key, loading: false, reloading: false });
      return;
    }
    const controller = new AbortController();
    let retryTimer: number | undefined;
    setState((s) => startLoad(s, key));
    loader
      .current(controller.signal)
      .then((data) => {
        if (controller.signal.aborted) return;
        attempt.current = 0;
        setState({ key, data, loading: false, reloading: false });
      })
      .catch((cause) => {
        if (controller.signal.aborted) return;
        const errorStatus = cause instanceof ApiError ? cause.status : undefined;
        const error = errorMessage(cause);
        setState((s) => ({
          ...s,
          key,
          error,
          errorStatus,
          loading: false,
          reloading: false,
          data: s.key === key ? s.data : undefined,
        }));
        if (autoRetryable(errorStatus)) {
          retryTimer = window.setTimeout(() => setTick((t) => t + 1), backoffDelay(attempt.current++));
        }
      });
    return () => {
      controller.abort();
      window.clearTimeout(retryTimer);
    };
    // `refresh` is a dependency only so that a new value reloads.
  }, [key, tick, refresh]);
  const reload = useCallback(() => {
    attempt.current = 0;
    setTick((t) => t + 1);
  }, []);
  // State left from an earlier key is not shown: a new key is loading until its first answer.
  const shown: Omit<ResourceState<T>, 'key'> = state.key === key ? state : { loading: !!key, reloading: false };
  return {
    data: shown.data,
    error: shown.error,
    errorStatus: shown.errorStatus,
    loading: shown.loading,
    reloading: shown.reloading,
    reload,
  };
}

/**
 * How long to wait before taking a new value, at most one every `ms`: none when the last one
 * was taken `ms` ago or more (or never), else the rest of that time.
 */
export function throttleWait(lastAt: number | null, now: number, ms: number): number {
  return lastAt == null ? 0 : Math.max(0, lastAt + ms - now);
}

/**
 * `value`, but taking a new value at most once every `ms`: the first change right away, later
 * ones at the end of the interval (the latest wins), so a burst of changes ends on the last one.
 */
export function useThrottled<T>(value: T, ms: number): T {
  const [shown, setShown] = useState(value);
  const lastAt = useRef<number | null>(null);
  useEffect(() => {
    if (Object.is(value, shown)) return;
    const take = () => {
      lastAt.current = Date.now();
      setShown(value);
    };
    const wait = throttleWait(lastAt.current, Date.now(), ms);
    if (wait === 0) {
      take();
      return;
    }
    const id = window.setTimeout(take, wait);
    return () => window.clearTimeout(id);
  }, [value, shown, ms]);
  return shown;
}

/** True once `flag` has stayed true for `ms` (a slow load), false as soon as it is false. */
export function useDelayedFlag(flag: boolean, ms: number): boolean {
  const [late, setLate] = useState(false);
  useEffect(() => {
    if (!flag) {
      setLate(false);
      return;
    }
    const id = window.setTimeout(() => setLate(true), ms);
    return () => window.clearTimeout(id);
  }, [flag, ms]);
  return flag && late;
}

/** Calls `fn` every `ms` while the page is visible. */
export function useInterval(fn: () => void, ms: number, enabled = true) {
  const saved = useRef(fn);
  saved.current = fn;
  useEffect(() => {
    if (!enabled) return;
    const id = window.setInterval(() => {
      if (document.visibilityState !== 'hidden') saved.current();
    }, ms);
    return () => window.clearInterval(id);
  }, [ms, enabled]);
}

export type LiveState = 'connecting' | 'live' | 'reconnecting' | 'off';

/** Reconnect delays: 1s, 2s, 4s … capped at 30s. */
export function backoffDelay(attempt: number): number {
  return Math.min(30_000, 1000 * 2 ** Math.max(0, attempt));
}

/**
 * Subscribes to a run's server-sent events: `changed` (new ledger events, debounced into one
 * refetch) and `history_error` (shown as a banner). Reconnects with backoff.
 */
export function useRunEvents(url: string | null, handlers: { changed: () => void; historyError: (message: string | null) => void }, debounceMs = 350): LiveState {
  const [state, setState] = useState<LiveState>(url ? 'connecting' : 'off');
  const saved = useRef(handlers);
  saved.current = handlers;
  useEffect(() => {
    if (!url || typeof EventSource === 'undefined') {
      setState('off');
      return;
    }
    let debounce: number | undefined;
    setState('connecting');
    const source = reconnectingEventSource(url, {
      onOpen: () => setState('live'),
      onRetry: () => setState('reconnecting'),
      events: {
        changed: () => {
          saved.current.historyError(null);
          window.clearTimeout(debounce);
          debounce = window.setTimeout(() => saved.current.changed(), debounceMs);
        },
        history_error: (event) => saved.current.historyError(historyErrorMessage(event)),
      },
    });
    return () => {
      source.close();
      window.clearTimeout(debounce);
    };
  }, [url, debounceMs]);
  return state;
}

/** The message of a `history_error` event, or a generic one when it carries none. */
export function historyErrorMessage(event: MessageEvent): string {
  try {
    const data = JSON.parse(event.data);
    if (typeof data.message === 'string') return data.message;
  } catch {
    // keep the generic message
  }
  return 'The run history could not be read.';
}

/**
 * An `EventSource` on `url` that, after an error, reconnects with `backoffDelay` until it is
 * closed. `onOpen` runs on every (re)connection, `onRetry` when a reconnection is scheduled.
 */
function reconnectingEventSource(url: string, options: { onOpen: () => void; onRetry: () => void; events: Record<string, (event: MessageEvent) => void> }): { close: () => void } {
  let source: EventSource | null = null;
  let retry: number | undefined;
  let attempt = 0;
  let closed = false;
  const connect = () => {
    if (closed) return;
    source = new EventSource(url);
    source.onopen = () => {
      attempt = 0;
      options.onOpen();
    };
    for (const [name, listener] of Object.entries(options.events)) {
      source.addEventListener(name, (event) => listener(event as MessageEvent));
    }
    source.onerror = () => {
      source?.close();
      if (closed) return;
      options.onRetry();
      retry = window.setTimeout(connect, backoffDelay(attempt++));
    };
  };
  connect();
  return {
    close: () => {
      closed = true;
      source?.close();
      window.clearTimeout(retry);
    },
  };
}

// Kept here for the pages that import it from hooks.
export { atBottom } from './browser';

/** A per-viewer UI preference kept in localStorage (layout widths, collapsed panes); falls back to `initial`. */
export function useStoredState<T>(key: string, initial: T): [T, (value: T) => void] {
  const [value, setValue] = useState<T>(() => {
    const raw = safeStorage.get(key);
    if (raw == null) return initial;
    try {
      return JSON.parse(raw) as T;
    } catch {
      return initial;
    }
  });
  const set = useCallback(
    (next: T) => {
      setValue(next);
      safeStorage.set(key, next === initial ? null : JSON.stringify(next));
    },
    [key, initial]
  );
  return [value, set];
}

/** Names the browser tab `title` (see route.ts `documentTitle`); `null` leaves it to another page. */
export function useDocumentTitle(title: string | null) {
  useEffect(() => {
    if (title != null) document.title = title;
  }, [title]);
}

/**
 * Prefixes every tab title with `(count) ` while `count` > 0, whatever page sets the rest: it
 * watches the <title> element and re-applies the prefix after each change.
 */
export function useTitleBadge(count: number) {
  useEffect(() => {
    const prefix = count > 0 ? `(${count}) ` : '';
    const apply = () => {
      const bare = document.title.replace(/^\(\d+\) /, '');
      if (document.title !== prefix + bare) document.title = prefix + bare;
    };
    apply();
    const title = document.querySelector('title');
    if (!title) return;
    const observer = new MutationObserver(apply);
    observer.observe(title, { childList: true, characterData: true, subtree: true });
    return () => observer.disconnect();
  }, [count]);
}
