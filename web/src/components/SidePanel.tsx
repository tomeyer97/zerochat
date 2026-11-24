import type { ReactNode } from 'react';
import { X } from 'lucide-react';

/**
 * The right-hand panel of the run page (a node, or the run's files): a header with the title,
 * whatever follows the title (a state pill), the close button and any header rows below it, then
 * the body as given.
 */
export function SidePanel({
  label,
  title,
  titleExtra,
  header,
  onClose,
  children,
}: {
  /** The panel's accessible name. */
  label: string;
  title: string;
  titleExtra?: ReactNode;
  /** Rows under the title row, inside the header (facts, round picker, tabs). */
  header?: ReactNode;
  onClose: () => void;
  children?: ReactNode;
}) {
  return (
    <aside className="node-panel" aria-label={label}>
      <header className="panel-header">
        <div className="panel-title-row">
          <h2 className="panel-title">{title}</h2>
          {titleExtra}
          <button type="button" className="icon-button panel-close" aria-label="Close (Esc)" title="Close (Esc)" onClick={onClose}>
            <X size={17} />
          </button>
        </div>
        {header}
      </header>
      {children}
    </aside>
  );
}
