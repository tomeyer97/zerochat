/** Where each tab-list key moves focus, given the focused tab's index and the number of tabs. */
const TAB_MOVES: Record<string, (from: number, count: number) => number> = {
  ArrowLeft: (from, count) => (from - 1 + count) % count,
  ArrowRight: (from, count) => (from + 1) % count,
  Home: () => 0,
  End: (_, count) => count - 1,
};

/** The tab a key moves to (wrapping at both ends), or null for keys a tab list ignores. */
export function nextTabIndex(key: string, from: number, count: number): number | null {
  const move = Object.hasOwn(TAB_MOVES, key) ? TAB_MOVES[key] : undefined;
  return move ? move(from, count) : null;
}

/** Roving-tabindex arrow-key navigation for a `role="tablist"` built from buttons/links tagged
 * `data-tab-index`: moves focus and activates the target tab, matching the ARIA tabs pattern. */
export function tabListKeyDown(event: React.KeyboardEvent<HTMLElement>, count: number, activate: (index: number) => void) {
  if (count < 2) return;
  const target = event.target as HTMLElement;
  const next = nextTabIndex(event.key, Number(target.dataset.tabIndex ?? -1), count);
  if (next === null) return;
  event.preventDefault();
  activate(next);
  const list = target.closest('[role="tablist"]');
  requestAnimationFrame(() => list?.querySelector<HTMLElement>(`[data-tab-index="${next}"]`)?.focus());
}
