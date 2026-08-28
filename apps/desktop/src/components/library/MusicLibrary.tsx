/**
 * Music Library (Phase 3.6). The forensic audit
 * (docs/phase-3-6-church-libraries.md) found no real, licensed production
 * song dataset anywhere in this repository - only a 5-song, explicitly
 * fictional development fixture (`docs/music-datasets.md`: "no real
 * hymnal or worship set has been imported or is claimed to be
 * installed"). Per this phase's hard rule ("never treat test fixtures as
 * production data" / "never hide an unavailable capability behind fake
 * data"), this view does not pretend a browsable song library exists.
 *
 * It honestly reports what content_registry actually has installed
 * (reusing the existing `listContentRegistry` command - no new backend),
 * and reuses the existing, fully-functional `searchMusic` command so an
 * operator can still search whatever *is* installed from a proper
 * top-level location instead of only from Diagnostics. A full song
 * browse/detail view was deliberately deferred - see
 * docs/phase-3-6-church-libraries.md's "Deferred work" section for why
 * building one against fictional data was judged not worth doing.
 */
import { useEffect, useState } from "react";
import type { ContentMetadata, MusicQueryType, SongRecognitionCandidate } from "../../domain";
import * as commands from "../../lib/commands";

export function MusicLibrary() {
  const [datasets, setDatasets] = useState<ContentMetadata[]>([]);
  const [query, setQuery] = useState("");
  const [queryType, setQueryType] = useState<MusicQueryType>("title");
  const [results, setResults] = useState<SongRecognitionCandidate[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    commands
      .listContentRegistry("music")
      .then(setDatasets)
      .catch((e) => setError(String(e)));
  }, []);

  const enabled = datasets.filter((d) => d.status === "enabled");
  const licensed = enabled.filter(
    (d) => d.licensingStatus === "verified_public_domain" || d.licensingStatus === "verified_redistributable" || d.licensingStatus === "licensed_for_cip",
  );

  const runSearch = () => {
    if (!query.trim()) return;
    setBusy("search");
    setError(null);
    commands
      .searchMusic(query.trim(), queryType)
      .then(setResults)
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(null));
  };

  return (
    <div className="library-page library-page--music">
      <header className="library-page__header">
        <div>
          <p className="library-page__eyebrow">Music Library</p>
          <h1>Songs</h1>
        </div>
      </header>

      {enabled.length === 0 ? (
        <p className="library-page__empty">
          No licensed song library installed yet. Music Intelligence can still detect and analyze available
          evidence during a live service (see Live Service &rarr; Music Intelligence in Diagnostics); a searchable
          song library will appear here once a licensed dataset is imported.
        </p>
      ) : (
        <>
          <p className="library-page__status-line">
            {enabled.length} music dataset{enabled.length === 1 ? "" : "s"} installed
            {licensed.length < enabled.length && " - not all have confirmed licensing"}.
          </p>
          <ul className="library-card-list">
            {enabled.map((d) => (
              <li key={d.id} className="library-card library-card--music">
                <div className="library-card__header">
                  <strong>{d.name}</strong>
                  <span className="library-card__meta">{d.licensingStatus.replace(/_/g, " ")}</span>
                </div>
                <p className="library-card__text">
                  {d.source}
                  {d.publisher ? ` · ${d.publisher}` : ""}
                </p>
              </li>
            ))}
          </ul>

          <section className="library-panel">
            <h2>Search</h2>
            <div className="live-brain__row">
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && runSearch()}
                placeholder="Search text"
                aria-label="Music search query"
              />
              <select value={queryType} onChange={(e) => setQueryType(e.target.value as MusicQueryType)} aria-label="Search type">
                <option value="title">Title/alias</option>
                <option value="number">Number</option>
                <option value="lyric">Lyric</option>
              </select>
              <button type="button" className="op-button--primary" disabled={!query.trim() || busy === "search"} onClick={runSearch}>
                Search
              </button>
            </div>
            {error && (
              <p className="live-brain__error" role="alert">
                {error}
              </p>
            )}
            {results && (
              <ul className="library-card-list">
                {results.length === 0 ? (
                  <li className="library-page__empty">No matches.</li>
                ) : (
                  results.map((r) => (
                    <li key={`${r.source}:${r.songId}`} className="library-card library-card--music">
                      <div className="library-card__header">
                        <strong>{r.explanation}</strong>
                        <span className="library-card__meta">{Math.round(r.confidence.score * 100)}%</span>
                      </div>
                      <p className="library-card__text">
                        {r.source} &middot; {r.matchType}
                      </p>
                    </li>
                  ))
                )}
              </ul>
            )}
          </section>
        </>
      )}
    </div>
  );
}
