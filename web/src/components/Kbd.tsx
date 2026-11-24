import type { ReactNode } from 'react';

/** A keyboard key, as in "press <Kbd>j</Kbd>". */
export function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="kbd">{children}</kbd>;
}
