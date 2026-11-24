import { useRef, type RefObject } from 'react';

/** Narrowest the side panel may get. */
const PANEL_MIN = 360;
/** Width the page column left of the divider always keeps. */
const PAGE_MIN = 320;
/** Page width assumed before the page has been measured. */
const FALLBACK_PAGE_WIDTH = 1200;
/** Panel width assumed before the panel has been measured. */
const FALLBACK_PANEL_WIDTH = 560;
/** How far one arrow-key press moves the divider. */
const KEY_STEP = 32;

/**
 * The divider between a page column and the side panel that follows it in the DOM: drag or arrow
 * keys to resize, double-click to reset. `width` is the stored width (null: the panel's own CSS
 * width); `pageRef` is the element both sit in, which bounds the drag.
 */
export function PanelResizer({ pageRef, width, onWidth }: { pageRef: RefObject<HTMLDivElement | null>; width: number | null; onWidth: (w: number | null) => void }) {
  const clamp = (w: number) => {
    const total = pageRef.current?.getBoundingClientRect().width ?? FALLBACK_PAGE_WIDTH;
    return Math.round(Math.max(PANEL_MIN, Math.min(total - PAGE_MIN, w)));
  };
  const divider = useRef<HTMLDivElement>(null);
  // The panel is the element right after this divider.
  const current = () => width ?? divider.current?.nextElementSibling?.getBoundingClientRect().width ?? FALLBACK_PANEL_WIDTH;
  return (
    <div
      ref={divider}
      className="panel-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize panel"
      aria-valuenow={Math.round(current())}
      tabIndex={0}
      title="Drag to resize · double-click to reset"
      onDoubleClick={() => onWidth(null)}
      onKeyDown={(e) => {
        if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
          e.preventDefault();
          onWidth(clamp(current() + (e.key === 'ArrowLeft' ? KEY_STEP : -KEY_STEP)));
        }
      }}
      onPointerDown={(e) => {
        const right = pageRef.current?.getBoundingClientRect().right;
        if (right == null) return;
        e.preventDefault();
        const handle = e.currentTarget;
        handle.setPointerCapture(e.pointerId);
        document.body.classList.add('resizing');
        const move = (ev: PointerEvent) => onWidth(clamp(right - ev.clientX));
        const up = () => {
          document.body.classList.remove('resizing');
          handle.removeEventListener('pointermove', move);
          handle.removeEventListener('pointerup', up);
          handle.removeEventListener('pointercancel', up);
        };
        handle.addEventListener('pointermove', move);
        handle.addEventListener('pointerup', up);
        handle.addEventListener('pointercancel', up);
      }}
    />
  );
}
