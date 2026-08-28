import { useEffect, useState } from "react";
import "./App.css";
import "./components/library/library.css";
import type { AppConfig } from "./config/appConfig";
import type { BibleTranslation } from "./domain";
import { appHealthCheck, getAppConfig, listBibleTranslations, type HealthReport } from "./lib/commands";
import { LiveChurchBrain } from "./components/LiveChurchBrain";
import { BibleLibrary } from "./components/library/BibleLibrary";
import { MusicLibrary } from "./components/library/MusicLibrary";
import { HistoryView } from "./components/library/HistoryView";
import { TestCenter } from "./components/testcenter/TestCenter";
import { WebRuntimeNotice } from "./components/WebRuntimeNotice";
import { isTauriRuntime } from "./lib/runtime";

interface FoundationState {
  config: AppConfig;
  health: HealthReport;
  translations: BibleTranslation[];
}

/**
 * Phase 3.6 top-level navigation (spec section 14). Deliberately the
 * smallest possible addition: local state, no router dependency, no
 * change to `LiveChurchBrain`'s internals - it renders exactly as it
 * always has, still the default and only thing on screen at launch. The
 * Libraries/History are separate, deeper-exploration destinations an
 * operator reaches on purpose, never controls dumped onto the live
 * workspace.
 */
type AppSection = "live" | "bible" | "music" | "history" | "test-center";

const SECTIONS: Array<{ id: AppSection; label: string }> = [
  { id: "live", label: "Live Service" },
  { id: "bible", label: "Bible" },
  { id: "music", label: "Music" },
  { id: "history", label: "History" },
  { id: "test-center", label: "Offline Test Center" },
];

function App() {
  const [state, setState] = useState<FoundationState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [section, setSection] = useState<AppSection>("live");
  // Read once per mount - the runtime a page loaded in does not change
  // during its lifetime.
  const [tauriRuntime] = useState(isTauriRuntime);

  useEffect(() => {
    if (!tauriRuntime) return;
    let cancelled = false;

    Promise.all([getAppConfig(), appHealthCheck(), listBibleTranslations()])
      .then(([config, health, translations]) => {
        if (!cancelled) setState({ config, health, translations });
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });

    return () => {
      cancelled = true;
    };
  }, [tauriRuntime]);

  if (!tauriRuntime) {
    return <WebRuntimeNotice />;
  }

  return (
    <>
      <nav className="app-nav" role="tablist" aria-label="CIP sections">
        {SECTIONS.map((s) => (
          <button key={s.id} type="button" aria-pressed={section === s.id} onClick={() => setSection(s.id)}>
            {s.label}
          </button>
        ))}
      </nav>

      {section === "live" && <LiveChurchBrain />}
      {section === "bible" && <BibleLibrary />}
      {section === "music" && <MusicLibrary />}
      {section === "history" && <HistoryView />}
      {section === "test-center" && <TestCenter />}

      <details className="foundation-details">
        <summary>Foundation status (Phase 1.0 diagnostics)</summary>
        <main className="foundation">
          {error && (
            <p className="error" role="alert">
              Failed to reach the backend: {error}
            </p>
          )}
          {!state && !error && <p>Connecting to backend&hellip;</p>}
          {state && (
            <div className="status-grid">
              <section>
                <h2>Environment</h2>
                <dl>
                  <dt>Mode</dt>
                  <dd>{state.config.environment}</dd>
                  <dt>Data directory</dt>
                  <dd className="path">{state.config.dataDir}</dd>
                  <dt>Database</dt>
                  <dd className="path">{state.config.databasePath}</dd>
                </dl>
              </section>

              <section>
                <h2>Database health</h2>
                <dl>
                  <dt>Connected</dt>
                  <dd>{state.health.databaseConnected ? "yes" : "no"}</dd>
                  <dt>Migrations applied</dt>
                  <dd>{state.health.appliedMigrations}</dd>
                </dl>
              </section>

              <section>
                <h2>Bible translations</h2>
                {state.translations.length === 0 ? (
                  <p>None installed (expected outside development).</p>
                ) : (
                  <ul>
                    {state.translations.map((t) => (
                      <li key={t.id}>
                        {t.name} ({t.abbreviation}) &mdash; {t.isLocal ? "local" : "remote"}
                      </li>
                    ))}
                  </ul>
                )}
              </section>
            </div>
          )}
        </main>
      </details>
    </>
  );
}

export default App;
