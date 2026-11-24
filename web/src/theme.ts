// The colour theme: follow the device (system), or force light or dark. The choice persists in
// localStorage, is shared between tabs (storage events), and is applied as
// `<html data-theme="light|dark">`, which the stylesheet keys on.
//
// The state lives in this module. `initializeTheme()` reads the stored choice and starts listening;
// main.tsx calls it before React mounts so the first paint has the right theme. It is idempotent,
// and `subscribeTheme`/`setThemePreference` call it too, so the module also works when nothing
// called it first.
export type ThemePreference = 'system' | 'light' | 'dark';

/** The localStorage key holding the preference (tools that preset a theme write it too). */
export const THEME_STORAGE_KEY = 'zerochat.theme.v1';

let preference: ThemePreference = 'system';
let media: MediaQueryList | undefined;
let initialized = false;
const listeners = new Set<() => void>();

function parsePreference(value: string | null): ThemePreference {
  return value === 'light' || value === 'dark' ? value : 'system';
}

function applyTheme() {
  document.documentElement.dataset.theme = preference === 'system' ? (media?.matches ? 'dark' : 'light') : preference;
}

function notify() {
  applyTheme();
  for (const listener of listeners) listener();
}

/** Call before mounting React so the first paint uses the device preference. */
export function initializeTheme() {
  if (initialized || typeof window === 'undefined' || typeof document === 'undefined') return;
  initialized = true;
  try {
    preference = parsePreference(window.localStorage.getItem(THEME_STORAGE_KEY));
  } catch {
    // Blocked storage must never prevent using the application.
  }
  if (typeof window.matchMedia === 'function') {
    media = window.matchMedia('(prefers-color-scheme: dark)');
    media.addEventListener('change', () => {
      if (preference === 'system') applyTheme();
    });
  }
  window.addEventListener('storage', (event) => {
    if (event.key !== THEME_STORAGE_KEY && event.key !== null) return;
    preference = parsePreference(event.newValue);
    notify();
  });
  applyTheme();
}

export function getThemePreference(): ThemePreference {
  return preference;
}

export function subscribeTheme(listener: () => void) {
  initializeTheme();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function setThemePreference(value: ThemePreference) {
  initializeTheme();
  preference = parsePreference(value);
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // Still usable for this page.
  }
  notify();
}
