/**
 * Local Congregant Companion View domain contracts (Phase 11). Mirrors
 * `apps/desktop/src-tauri/src/companion.rs`'s `CompanionStatus` and the
 * `CompanionStatusDto` wire format in `commands.rs`. See
 * `docs/phase-11-audit.md` and `docs/congregant-companion.md` for the
 * full design reasoning.
 */

export interface CompanionStatus {
  running: boolean;
  port: number;
  /** Candidate `http://` URLs a phone on the same LAN could open - may be empty. */
  urls: string[];
}
