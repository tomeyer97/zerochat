// The run page's keyboard shortcuts.
import { useEffect, useRef } from 'react';

/** True when keys typed at `el` are text (an input, a text area, a select, editable content). */
export function isTypingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  return ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName) || el.isContentEditable;
}

/**
 * The run page's keys, listened for on the whole window:
 *   Escape  closes the side panel (`onClose`, only when a panel is open)
 *   j / k   step to the next / previous execution (`onStep(+1 | -1)`)
 * None of them fires while the user is typing, while `enabled` is false (a dialog is open), while
 * any modal <dialog> is open (the Glossary is not the run page's own), or with a modifier held (j/k).
 */
export function useRunShortcuts({ enabled, panelOpen, onClose, onStep }: { enabled: boolean; panelOpen: boolean; onClose: () => void; onStep: (delta: 1 | -1) => boolean }) {
  // The latest callbacks, so the listener is added once rather than on every render.
  const latest = useRef({ enabled, panelOpen, onClose, onStep });
  latest.current = { enabled, panelOpen, onClose, onStep };
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const { enabled, panelOpen, onClose, onStep } = latest.current;
      if (!enabled || document.querySelector('dialog[open]')) return;
      const typing = isTypingTarget(event.target);
      if (event.key === 'Escape' && panelOpen && !typing) {
        event.preventDefault();
        onClose();
        return;
      }
      if (typing || event.metaKey || event.ctrlKey || event.altKey) return;
      if ((event.key === 'j' || event.key === 'k') && onStep(event.key === 'j' ? 1 : -1)) event.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
}
