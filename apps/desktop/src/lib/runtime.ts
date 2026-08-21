/**
 * Runtime detection: is this frontend running inside the Tauri desktop
 * shell (CIP Desktop), or as a normal web page in an ordinary browser
 * (e.g. this same frontend deployed to Vercel and opened on a phone)?
 *
 * `@tauri-apps/api`'s `invoke`/`listen` reach into `window.__TAURI_INTERNALS__`,
 * which only exists inside a real Tauri WebView. Calling them outside one
 * throws `TypeError: Cannot read properties of undefined (reading 'invoke')`
 * - not something CIP should ever surface as a raw exception to someone
 * who opened the web deployment in a browser. Every IPC call
 * (`lib/commands.ts`) and event subscription (`lib/liveEvents.ts`) is
 * gated behind {@link isTauriRuntime} instead - see
 * `docs/live-speech.md`'s "CIP Web" section.
 */
import { isTauri } from "@tauri-apps/api/core";

export function isTauriRuntime(): boolean {
  try {
    return isTauri();
  } catch {
    // Defensive only - `isTauri()` just reads a global boolean and does
    // not throw in practice, but a raw exception here would defeat the
    // entire point of this check.
    return false;
  }
}
