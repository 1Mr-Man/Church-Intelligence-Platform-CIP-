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
import type {
  BibleDetectionAnalytics,
  ContentCandidate,
  PresentationItem,
  SermonKnowledgeBase,
  ServiceReport,
  ServiceSession,
  Suggestion,
  TimelineEntry,
  TranscriptSegment,
} from "../../domain";
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
  const [report, setReport] = useState<ServiceReport | null>(null);
  const [knowledgeBase, setKnowledgeBase] = useState<SermonKnowledgeBase | null>(null);
  const [accuracy, setAccuracy] = useState<BibleDetectionAnalytics | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    commands
      .listServiceHistory(20)
      .then(setServices)
      .catch((e) => setError(String(e)));
    // Church Knowledge Base (Phase 13) deliberately spans every service,
    // not just the one an operator selects below - loaded once, not tied
    // to `openService`.
    commands
      .getChurchKnowledgeBase()
      .then(setKnowledgeBase)
      .catch((e) => setError(String(e)));
    // Detection Accuracy Analytics (Phase 17) likewise spans every
    // service, not the one an operator selects below.
    commands
      .getBibleDetectionAnalytics()
      .then(setAccuracy)
      .catch((e) => setError(String(e)));
  }, []);

  const openService = (service: ServiceSession) => {
    setSelected(service);
    setError(null);
    setNotice(null);
    setReport(null);
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
    // Kept separate from the Promise.all above: a report failure (e.g. a
    // pre-Phase-5.1 service with no report support) must never block the
    // rest of the history view from loading.
    commands
      .getServiceReport(service.id)
      .then(setReport)
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
        <>
          <section className="library-panel">
            <h2>Detection Accuracy</h2>
            <p className="live-brain__hint">
              How often an operator has approved, edited, or rejected a Bible suggestion, across every service - not
              just the one you open below.
            </p>
            {!accuracy || accuracy.overall.total === 0 ? (
              <p className="library-page__empty">
                Nothing decided yet - approve, edit, or reject a Bible suggestion during a service to start building
                this.
              </p>
            ) : (
              <>
                <ul className="library-card-list">
                  <li className="library-card library-card--bible">
                    <div className="library-card__header">
                      <strong>Overall</strong>
                      <span className="library-card__meta">
                        {accuracy.overallApprovalRate !== null
                          ? `${(accuracy.overallApprovalRate * 100).toFixed(0)}% approved`
                          : "nothing decided yet"}
                      </span>
                    </div>
                    <p className="library-card__text">
                      {accuracy.overall.total} total &middot; {accuracy.overall.approved} approved &middot;{" "}
                      {accuracy.overall.edited} edited &middot; {accuracy.overall.rejected} rejected &middot;{" "}
                      {accuracy.overall.pending} pending
                      {accuracy.rejectionEchoes > 0 && (
                        <>
                          {" "}
                          &middot; {accuracy.rejectionEchoes} rejected reference{accuracy.rejectionEchoes === 1 ? "" : "s"}{" "}
                          redetected again and kept suppressed
                        </>
                      )}
                    </p>
                  </li>
                  <li className="library-card library-card--bible">
                    <div className="library-card__header">
                      <strong>By confidence level</strong>
                    </div>
                    <p className="library-card__text">
                      {accuracy.byConfidenceLevel
                        .filter((b) => b.counts.total > 0)
                        .map((b) => {
                          const rate =
                            b.counts.approved + b.counts.edited + b.counts.rejected > 0
                              ? Math.round(
                                  (b.counts.approved / (b.counts.approved + b.counts.edited + b.counts.rejected)) * 100,
                                )
                              : null;
                          return `${b.level}: ${b.counts.total}${rate !== null ? ` (${rate}% approved)` : ""}`;
                        })
                        .join(" · ") || "No decided suggestions yet."}
                    </p>
                  </li>
                  {accuracy.byDetectionKind.length > 0 && (
                    <li className="library-card library-card--bible">
                      <div className="library-card__header">
                        <strong>By detection method</strong>
                      </div>
                      <p className="library-card__text">
                        {accuracy.byDetectionKind
                          .map((b) => {
                            const rate =
                              b.counts.approved + b.counts.edited + b.counts.rejected > 0
                                ? Math.round(
                                    (b.counts.approved / (b.counts.approved + b.counts.edited + b.counts.rejected)) *
                                      100,
                                  )
                                : null;
                            return `${b.kind.replace(/_/g, " ")}: ${b.counts.total}${rate !== null ? ` (${rate}% approved)` : ""}`;
                          })
                          .join(" · ")}
                      </p>
                      {accuracy.unmatchedDetectionKindCount > 0 && (
                        <p className="live-brain__hint">
                          {accuracy.unmatchedDetectionKindCount} suggestion
                          {accuracy.unmatchedDetectionKindCount === 1 ? "" : "s"} could not be matched to a detection
                          method (e.g. a manually corrected reference) and are not counted above.
                        </p>
                      )}
                    </li>
                  )}
                </ul>
                {accuracy.serviceTrend.filter((s) => s.counts.total > 0).length > 1 && (
                  <details>
                    <summary>Trend by service (oldest first)</summary>
                    <ul className="library-card-list">
                      {accuracy.serviceTrend
                        .filter((s) => s.counts.total > 0)
                        .map((s) => {
                          const decided = s.counts.approved + s.counts.edited + s.counts.rejected;
                          const rate = decided > 0 ? Math.round((s.counts.approved / decided) * 100) : null;
                          return (
                            <li key={s.serviceId} className="library-card library-card--bible">
                              <div className="library-card__header">
                                <strong>{s.serviceTitle}</strong>
                                <span className="library-card__meta">
                                  {rate !== null ? `${rate}% approved` : "nothing decided"}
                                </span>
                              </div>
                              <p className="library-card__text">
                                {formatClockTime(s.startedAt)} &middot; {s.counts.total} total &middot;{" "}
                                {s.counts.approved} approved &middot; {s.counts.edited} edited &middot;{" "}
                                {s.counts.rejected} rejected
                              </p>
                            </li>
                          );
                        })}
                    </ul>
                  </details>
                )}
              </>
            )}
          </section>

          <section className="library-panel">
            <h2>Church Knowledge Base</h2>
            <p className="live-brain__hint">
              Themes, speakers, and findings an operator has explicitly accepted, gathered from every service - not just
              the one you open below. See {knowledgeBase?.recentFindings.length ?? 0} accepted finding
              {knowledgeBase?.recentFindings.length === 1 ? "" : "s"} so far.
            </p>
            {!knowledgeBase || (knowledgeBase.themeFrequency.length === 0 && knowledgeBase.sermonsBySpeaker.length === 0) ? (
              <p className="library-page__empty">
                Nothing here yet - accept a Sermon Intelligence finding during a service to start building this.
              </p>
            ) : (
              <>
                {knowledgeBase.themeFrequency.length > 0 && (
                  <div>
                    <h3>Most-preached themes</h3>
                    <ul className="library-card-list">
                      {knowledgeBase.themeFrequency.map((t) => (
                        <li key={t.label} className="library-card library-card--bible">
                          <div className="library-card__header">
                            <strong>{t.label}</strong>
                            <span className="library-card__meta">
                              {t.occurrenceCount} mention{t.occurrenceCount === 1 ? "" : "s"} across {t.sermonCount} sermon
                              {t.sermonCount === 1 ? "" : "s"}
                            </span>
                          </div>
                          {t.sermons.length > 0 && (
                            <p className="library-card__text">
                              {t.sermons.map((s) => s.title ?? "Untitled sermon").join(" · ")}
                            </p>
                          )}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
                {knowledgeBase.sermonsBySpeaker.length > 0 && (
                  <div>
                    <h3>Sermons by speaker</h3>
                    <ul className="library-card-list">
                      {knowledgeBase.sermonsBySpeaker.map((sp) => (
                        <li key={sp.speakerName} className="library-card library-card--bible">
                          <div className="library-card__header">
                            <strong>{sp.speakerName}</strong>
                            <span className="library-card__meta">
                              {sp.sermonCount} sermon{sp.sermonCount === 1 ? "" : "s"}
                            </span>
                          </div>
                          <p className="library-card__text">
                            {sp.sermons.map((s) => s.title ?? "Untitled sermon").join(" · ")}
                          </p>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </>
            )}
          </section>

          {services.length === 0 ? (
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
          )}
        </>
      ) : (
        <>
          {report && (
            <section className="library-panel">
              <h2>Service Report</h2>
              <p className="library-card__meta">
                {report.durationMinutes !== null
                  ? `Duration: ${report.durationMinutes.toFixed(1)} min`
                  : "Duration: service still active"}
              </p>
              <ul className="library-card-list">
                <li className="library-card library-card--bible">
                  <div className="library-card__header">
                    <strong>Suggestions</strong>
                  </div>
                  <p className="library-card__text">
                    {report.suggestionStats.total} total &middot; {report.suggestionStats.approved} approved &middot;{" "}
                    {report.suggestionStats.edited} edited &middot; {report.suggestionStats.rejected} rejected &middot;{" "}
                    {report.suggestionStats.pending} pending
                    {report.suggestionStats.rejectionEchoes > 0 && (
                      <>
                        {" "}
                        &middot; {report.suggestionStats.rejectionEchoes} rejected reference
                        {report.suggestionStats.rejectionEchoes === 1 ? "" : "s"} redetected again and kept suppressed
                      </>
                    )}
                  </p>
                </li>
                {report.detectionKindCounts.length > 0 && (
                  <li className="library-card library-card--bible">
                    <div className="library-card__header">
                      <strong>Detections by kind</strong>
                    </div>
                    <p className="library-card__text">
                      {report.detectionKindCounts.map((d) => `${d.kind.replace(/_/g, " ")}: ${d.count}`).join(" · ")}
                    </p>
                  </li>
                )}
                {report.timelineCategoryCounts.length > 0 && (
                  <li className="library-card library-card--bible">
                    <div className="library-card__header">
                      <strong>Timeline by category</strong>
                    </div>
                    <p className="library-card__text">
                      {report.timelineCategoryCounts.map((c) => `${c.category}: ${c.count}`).join(" · ")}
                    </p>
                  </li>
                )}
              </ul>
              <details>
                <summary>Live pipeline diagnostics (since app launch, not this service alone)</summary>
                <p className="live-brain__hint">
                  These counters accumulate across every service run in this session of the app - they are not scoped
                  to this service alone.
                </p>
                <ul className="library-card-list">
                  <li className="library-card library-card--bible">
                    <p className="library-card__text">
                      Speech: {report.liveDiagnostics.speechModelLoaded ? "model loaded" : "model not loaded"} &middot;{" "}
                      {report.liveDiagnostics.inferencesSucceeded}/{report.liveDiagnostics.inferencesAttempted} inferences
                      succeeded
                      {report.liveDiagnostics.avgInferenceDurationMs !== null &&
                        ` · avg ${report.liveDiagnostics.avgInferenceDurationMs}ms`}
                      {report.liveDiagnostics.overloadEvents > 0 && ` · ${report.liveDiagnostics.overloadEvents} overload events`}
                    </p>
                    <p className="library-card__text">
                      Semantic search:{" "}
                      {report.liveDiagnostics.embeddingReady ? "ready" : "not ready"}
                      {!report.liveDiagnostics.embeddingFeatureCompiled && " (not compiled into this build)"}
                    </p>
                  </li>
                </ul>
              </details>
            </section>
          )}

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
                      <span className="library-card__meta">
                        {s.status}
                        {s.rejectionEchoCount > 0 && ` · echoed ×${s.rejectionEchoCount}`}
                      </span>
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
