/** Local `HH:MM:SS` for a stored UTC ISO-8601 timestamp - used by the
 * live transcript and service timeline so the operator sees times in
 * their own clock, not raw ISO strings. */
export function formatClockTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleTimeString(undefined, { hour12: false });
}
