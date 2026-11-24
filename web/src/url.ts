/** `?a=1&b=2` from the params that have a value, or `''` when none has; values are URL-encoded. */
export function queryString(params: Record<string, string | null | undefined>): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) if (value) search.set(key, value);
  const text = search.toString();
  return text ? `?${text}` : '';
}
