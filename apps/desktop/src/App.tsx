import { useEffect, useState } from "react";
import "./App.css";
import type { AppConfig } from "./config/appConfig";
import type { BibleTranslation } from "./domain";
import { appHealthCheck, getAppConfig, listBibleTranslations, type HealthReport } from "./lib/commands";
import { LiveChurchBrain } from "./components/LiveChurchBrain";

interface FoundationState {
  config: AppConfig;
  health: HealthReport;
  translations: BibleTranslation[];
}

function App() {
  const [state, setState] = useState<FoundationState | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
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
  }, []);

  return (
    <>
      <LiveChurchBrain />

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
