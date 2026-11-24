import type { ReactNode } from 'react';
import { LoaderCircle } from 'lucide-react';

/**
 * A small action button that spins while its action (`name`) is the busy one; all wait while any
 * is. `spinning` overrides when it spins (another control may be the one sending the action).
 */
export function ActionButton<Name extends string>({
  name,
  busy,
  primary = false,
  spinning = busy === name,
  title,
  onClick,
  children,
}: {
  name: Name;
  busy: Name | null;
  primary?: boolean;
  spinning?: boolean;
  title?: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button type="button" className={primary ? 'button small primary' : 'button small'} disabled={!!busy} onClick={onClick} title={title}>
      {spinning && <LoaderCircle size={13} className="spin" aria-hidden />} {children}
    </button>
  );
}
