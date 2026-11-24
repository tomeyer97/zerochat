// Small helpers over browser APIs that the pages share (not hooks: plain functions).

/** True while the element is scrolled to (near) its bottom. */
export function atBottom(element: HTMLElement, slack = 48): boolean {
  return element.scrollHeight - element.scrollTop - element.clientHeight < slack;
}

/**
 * localStorage that never throws: private windows and blocked storage refuse access, and a
 * preference that cannot be read or kept must never stop the page from working.
 */
export const safeStorage = {
  /** The stored value, or `null` when there is none or storage is blocked. */
  get(key: string): string | null {
    try {
      return window.localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  /** Stores `value` (`null` removes the key); does nothing when storage is blocked. */
  set(key: string, value: string | null): void {
    try {
      if (value == null) window.localStorage.removeItem(key);
      else window.localStorage.setItem(key, value);
    } catch {
      // The preference just doesn't persist.
    }
  },
};
