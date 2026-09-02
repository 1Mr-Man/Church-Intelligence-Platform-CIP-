/**
 * Phase 6.5 (Operator Ergonomics: error banner dismiss/context) - turns a
 * `withBusy` busy-state key (e.g. `"approve-00000000-0000-0000-0000-
 * 000000000001"`, `"start-service"`, `` `cross-domain-dismiss-${id}` ``)
 * into a short, human-readable label for the error banner, so an
 * operator can tell *which* action failed instead of only that
 * *something* did. Deliberately mechanical rather than a lookup table
 * for ~40+ call sites: strips a trailing UUID (the common id-suffix
 * shape most keys have), turns the remaining hyphens into spaces, and
 * capitalizes the first letter - readable, not polished English, but
 * genuinely more useful than nothing and correct for every existing key
 * without needing a new label added at every call site.
 */
const TRAILING_UUID = /-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function humanizeBusyKey(key: string): string {
  const withoutId = key.replace(TRAILING_UUID, "");
  const spaced = withoutId.replace(/-/g, " ");
  if (spaced.length === 0) return spaced;
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}
