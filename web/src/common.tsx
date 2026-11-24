/**
 * Shared UI widgets. Each lives in its own module under components/; this root re-exports them so
 * `import … from './common'` keeps working:
 *   components/a11y.ts             tabListKeyDown: arrow-key navigation for a tab list
 *   components/StatusBadges.tsx    StatusDot, StatusPill (a run), StateIcon, StatePill (a node)
 *   components/feedback.tsx        ErrorBox, InfoBanner, Skeleton, EmptyState
 *   components/ErrorBoundary.tsx   ErrorBoundary: the app-level render-crash fallback
 *   components/CopyButton.tsx      CopyButton (components/clipboard.ts: copyText)
 *   components/Modal.tsx           Modal, on the native <dialog>
 *   components/LongText.tsx        LongText: a collapsible long text
 *   components/Kbd.tsx             Kbd: a keyboard key
 *   components/cx.ts               cx: a className from conditional parts
 * Also in components/, imported directly: ConfirmDialog, PanelResizer, ThemeControl, graph-layout.
 */
export { tabListKeyDown } from './components/a11y';
export { StatusDot, StatusPill, StateIcon, StatePill } from './components/StatusBadges';
export { ErrorBox, InfoBanner, Skeleton, EmptyState } from './components/feedback';
export { ErrorBoundary } from './components/ErrorBoundary';
export { CopyButton } from './components/CopyButton';
export { Modal } from './components/Modal';
export { LongText } from './components/LongText';
export { Kbd } from './components/Kbd';
export { cx } from './components/cx';
