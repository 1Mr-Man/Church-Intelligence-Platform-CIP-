import "./WebRuntimeNotice.css";

/**
 * Shown instead of the Live Church Brain / foundation diagnostics when
 * this frontend is running in a normal web browser rather than the Tauri
 * desktop shell (e.g. the web deployment opened directly, outside CIP
 * Desktop). CIP Web has no Rust backend, no local SQLite database, no
 * audio/speech engine - every feature this build offers requires Tauri
 * IPC, so there is nothing safe to attempt here yet. See
 * `docs/live-speech.md`'s "CIP Web" section.
 */
export function WebRuntimeNotice() {
  return (
    <div className="web-runtime-notice">
      <h1>Church Intelligence Platform</h1>
      <p className="web-runtime-notice__badge">Web Runtime</p>
      <p>
        You&rsquo;re viewing CIP in a web browser. Live service features - audio capture, speech recognition,
        scripture detection, and the suggestion review workflow - run through a local Rust backend and SQLite
        database that only exist inside the <strong>CIP Desktop</strong> application.
      </p>
      <p className="web-runtime-notice__hint">
        Install and run CIP Desktop to use these features. This page will not attempt to contact a desktop backend
        from here.
      </p>
    </div>
  );
}
