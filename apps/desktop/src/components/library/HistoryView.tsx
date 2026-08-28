/**
 * Service & Presentation History (Phase 3.6). Every fact shown here
 * already existed and already survived a restart before this phase - see
 * docs/phase-3-6-church-libraries.md's Service/Presentation History
 * audit. The only new backend surface is `listPresentationHistory`, a
 * thin wrapper exposing the existing `presentation_items` table for an
 * arbitrary (not just the live) service; everything else
 * (`listServiceHistory`/`getService`/`listTimeline`/`listTranscript`/
 * `listSuggestions`, all already `serviceId`-aware) is reused exactly as
 * it already worked.
 *
 * "Reuse" never mutates a historical record: it calls the same
 * `createManualPresentation` command the Bible Library uses, which always
 * creates a brand-new presentation item for whatever service is currently
 * live - the historical item this button reads from is never touched.
 */
import { useEffect, useState } from "react";
import type { ContentCandidate, PresentationItem, ServiceSession, Suggestion, TimelineEntry, TranscriptSegment } from "../../domain";
import * as commands from "../../lib/commands";
import { formatClockTime } from "../../lib/format";
import { presentationHeading } from "../../lib/libraryHelpers";
import { describeTimelineEntry } from "../../lib/timelineFormat";

export function HistoryView() {
  const [services, setServices] = useState<ServiceSession[]>([]);
  const [selected, setSelected] = useState<ServiceSession | null>(null);
  const [timeline, setTimeline] = useState<TimelineEntry[]>([]);
  const [transcript, setTranscript] = useState<TranscriptSegment[]>([]);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [presentations, setPresentations] = useState<PresentationItem[]>([]);
  const [savedContent, setSavedContent] = useState<ContentCandidate[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    commands
      .listServiceHistory(20)
      .then(setServices)
      .catch((e) => setError(String(e)));
  }, []);

  const openService = (service: ServiceSession) => {
    setSelected(service);
    setError(null);
    setNotice(null);
    Promise.all([
      commands.listTimeline(100, service.id),
      commands.listTranscript(200, service.id),
      commands.listSuggestions(undefined, service.id),
      commands.listPresentationHistory(service.id),
      commands.listSavedContent(service.id),
    ])
      .then(([t, tr, s, p, c]) => {
        setTimeline(t);
        setTranscript(tr);
        setSuggestions(s);
        setPresentations(p);
        setSavedContent(c);
      })
      .catch((e) => setError(String(e)));
  };

  const reuse = (item: PresentationItem) => {
    if (item.content.type !== "scripture") return;
    const key = `reuse-${item.id}`;
    setBusy(key);
    setError(null);
    commands
      .createManualPresentation(item.content.reference, item.content.translationId)
      .then(() => setNotice(`Prepared ${item.content.type === "scripture" ? item.content.reference : ""} for the current live service.`))
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(null));
  };

  return (
    <div className="library-page library-page--history">
      <header className="library-page__header">
        <div>
          <p className="library-page__eyebrow">History</p>
          <h1>{selected ? selected.title : "Past Services"}</h1>
        </div>
        {selected && (
          <button type="button" onClick={() => setSelected(null)}>
            &larr; All services
          </button>
        )}
      </header>

      {error && (
        <p className="live-brain__error" role="alert">
          {error}
        </p>
      )}
      {notice && <p className="library-page__notice">{notice}</p>}

      {!selected ? (
        services.length === 0 ? (
          <p className="library-page__empty">
            No completed services yet. Your service history will appear here after the first completed service.
          </p>
        ) : (
          <ul className="library-card-list">
            {services.map((s) => (
              <li key={s.id} className="library-card library-card--history">
                <div className="library-card__header">
                  <strong>{s.title}</strong>
                  <span className="library-card__meta">{s.status}</span>
                </div>
                <p className="library-card__text">
                  {formatClockTime(s.startedAt)}
                  {s.endedAt ? ` – ${formatClockTime(s.endedAt)}` : ""}
                </p>
                <div className="library-card__actions">
                  <button type="button" className="op-button--primary" onClick={() => openService(s)}>
                    Open
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )
      ) : (
        <>
          <section className="library-panel">
            <h2>Presentation History ({presentations.length})</h2>
            {presentations.length === 0 ? (
              <p className="live-brain__hint">Nothing was prepared or displayed during this service.</p>
            ) : (
              <ul className="library-card-list">
                {presentations.map((item) => (
                  <li key={item.id} className="library-card library-card--bible">
                    <div className="library-card__header">
                      <strong>{presentationHeading(item)}</strong>
                      <span className="library-card__meta">{item.status}</span>
                    </div>
                    {item.content.type === "scripture" && <p className="library-card__text">{item.content.text}</p>}
                    {item.content.type === "scripture" && (
                      <div className="library-card__actions">
                        <button type="button" disabled={busy === `reuse-${item.id}`} onClick={() => reuse(item)}>
                          Reuse in current service
                        </button>
                      </div>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="library-panel">
            <h2>Saved Content ({savedContent.length})</h2>
            {savedContent.length === 0 ? (
              <p className="live-brain__hint">Nothing was accepted as content from this service.</p>
            ) : (
              <ul className="library-card-list">
                {savedContent.map((c) => (
                  <li key={c.id} className="library-card library-card--bible">
                    <div className="library-card__header">
                      <strong>{c.titleOrLabel}</strong>
                      <span className="library-card__meta">{c.candidateType.replace(/_/g, " ")}</span>
                    </div>
                    <p className="library-card__text">{c.workingConcept}</p>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="library-panel">
            <h2>Scripture &amp; Findings ({suggestions.length})</h2>
            {suggestions.length === 0 ? (
              <p className="live-brain__hint">No detections recorded for this service.</p>
            ) : (
              <ul className="library-card-list">
                {suggestions.map((s) => (
                  <li key={s.id} className="library-card library-card--bible">
                    <div className="library-card__header">
                      <strong>{s.kind.type === "scripture" ? s.kind.reference : s.kind.label}</strong>
                      <span className="library-card__meta">{s.status}</span>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <details className="library-panel">
            <summary>Transcript ({transcript.length})</summary>
            <ul className="live-brain__transcript">
              {transcript.map((seg) => (
                <li key={seg.id}>&ldquo;{seg.text}&rdquo;</li>
              ))}
            </ul>
          </details>

          <details className="library-panel">
            <summary>Timeline ({timeline.length})</summary>
            <ul className="live-brain__timeline">
              {timeline.map((entry) => (
                <li key={entry.id}>
                  <span className="live-brain__timestamp">{formatClockTime(entry.createdAt)}</span>
                  {describeTimelineEntry(entry)}
                </li>
              ))}
            </ul>
          </details>
        </>
      )}
    </div>
  );
}
