/** A `className` from its parts: the truthy ones, space-separated (`cx('app', open && 'drawer-open')`). */
export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(' ');
}
