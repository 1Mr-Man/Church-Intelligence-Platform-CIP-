import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AudioDevice,
  BibleSearchResult,
  BibleTranslation,
  ContentMetadata,
  DomainCapabilityReport,
  ImportReport,
  IntegrityReport,
  IntelligenceFinding,
  LiveStatus,
  MusicQueryType,
  PresentationItem,
  PresentationPreview,
  ScriptureContext,
  ScriptureDetection,
  ScriptureReference,
  ServiceSession,
  SermonStateSnapshot,
  SongRecognitionCandidate,
  Suggestion,
  TimelineEntry,
  TranscriptSegment,
} from "../domain";
import * as commands from "../lib/commands";
import * as liveEvents from "../lib/liveEvents";
import { formatClockTime } from "../lib/format";
import { describeTimelineEntry } from "../lib/timelineFormat";
import { shouldHandleShortcut } from "../lib/keyboardShortcuts";
import "./LiveChurchBrain.css";

const STATUS_POLL_MS = 3000;
const TRANSCRIPT_LIMIT = 20;
const TIMELINE_LIMIT = 50;
const RECENT_REFERENCES_LIMIT = 8;
const AMBIGUOUS_LIMIT = 5;

function referenceDisplay(ref: ScriptureReference): string {
  return `${ref.book} ${ref.chapter}:${ref.verseStart}`;
}

/**
 * Live Church Brain - the operator workspace (Phase 1.3). Deliberately
 * keeps CURRENT (active context / most recent reference) distinct from
 * RECENT (a bounded list) distinct from HISTORY (the full service
 * timeline, and the separate service archive) - see
 * `docs/live-service.md`'s "current/recent/history" section for why these
 * are three different things, not one collapsed "what's happening" view.
 * There is still no projected-content control anywhere here: preparing a
 * presentation item is as far as this phase goes.
 */
export function LiveChurchBrain() {
  const [status, setStatus] = useState<LiveStatus | null>(null);
  const [activeContext, setActiveContext] = useState<ScriptureContext | null>(null);
  const [lastReference, setLastReference] = useState<ScriptureReference | null>(null);
  const [recentReferences, setRecentReferences] = useState<ScriptureReference[]>([]);
  const [ambiguous, setAmbiguous] = useState<ScriptureDetection[]>([]);
  const [transcript, setTranscript] = useState<TranscriptSegment[]>([]);
  // `TranscriptSegment` carries no wall-clock timestamp (`startMs`/`endMs`
  // are audio-relative, not clock time - see its domain doc comment), so
  // this records the one honest timestamp available: when this frontend
  // actually received the segment. A segment loaded from persisted
  // history (before any live event fired) has no entry here and simply
  // shows no time, rather than a fabricated one.
  const [transcriptReceivedAt, setTranscriptReceivedAt] = useState<Record<string, string>>({});
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  // Approved but not yet prepared - a suggestion moves here (never
  // disappears) on approval, and out again once prepared. Kept distinct
  // from `suggestions` (pending/edited) because only an approved
  // suggestion may ever be prepared - see `ensure_suggestion_approved` on
  // the Rust side and Phase 1.4 section 3/16.
  const [approvedSuggestions, setApprovedSuggestions] = useState<Suggestion[]>([]);
  // Currently prepared presentation items for the active service - the
  // Current Output panel's data (Phase 1.4 section 27). Never includes
  // cancelled items.
  const [preparedItems, setPreparedItems] = useState<PresentationItem[]>([]);
  // Non-mutating previews, keyed by suggestion id / search reference -
  // Preview never changes `suggestions`/`approvedSuggestions`/`preparedItems`
  // (section 14: preview and prepare are separate actions).
  const [previews, setPreviews] = useState<Record<string, PresentationPreview>>({});
  const [searchPreviews, setSearchPreviews] = useState<Record<string, PresentationPreview>>({});
  const [timeline, setTimeline] = useState<TimelineEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [serviceTitle, setServiceTitle] = useState("Sunday Morning Service");
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [selectedDevice, setSelectedDevice] = useState("");
  const [manualText, setManualText] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<BibleSearchResult[]>([]);
  const [translations, setTranslations] = useState<BibleTranslation[]>([]);
  const [searchTranslation, setSearchTranslation] = useState("");
  // Content Registry (Phase 1.5) - not service-scoped, so this loads once
  // on mount and refreshes after any action that changes it.
  const [contentItems, setContentItems] = useState<ContentMetadata[]>([]);
  const [integrityByTranslation, setIntegrityByTranslation] = useState<Record<string, IntegrityReport>>({});
  const [importResult, setImportResult] = useState<ImportReport | null>(null);
  const [importFileName, setImportFileName] = useState<string | null>(null);
  // Intelligence Status (Phase 2.0) - a diagnostic-only view of which
  // intelligence domains have a real engine registered. Not
  // service-scoped: loads once on mount.
  const [intelligenceCapabilities, setIntelligenceCapabilities] = useState<DomainCapabilityReport[]>([]);
  // Music Intelligence (Phase 2.1) - findings await operator review the
  // same way suggestions do, but never auto-prepare a presentation item
  // (see docs/music-intelligence.md's hard requirement). Service-scoped,
  // like suggestions/transcript above.
  const [musicFindings, setMusicFindings] = useState<IntelligenceFinding[]>([]);
  const [musicManualText, setMusicManualText] = useState("");
  const [musicSearchQuery, setMusicSearchQuery] = useState("");
  const [musicSearchType, setMusicSearchType] = useState<MusicQueryType>("title");
  const [musicSearchResults, setMusicSearchResults] = useState<SongRecognitionCandidate[] | null>(null);
  // Sermon Intelligence (Phase 2.3) - findings await operator review the
  // same way Music findings do, never auto-projected. `sermonState` is
  // the read-only theme/state/structure snapshot, independent of finding
  // review status (see docs/sermon-intelligence.md).
  const [sermonFindings, setSermonFindings] = useState<IntelligenceFinding[]>([]);
  const [sermonState, setSermonState] = useState<SermonStateSnapshot | null>(null);
  const [sermonManualText, setSermonManualText] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [correctionBook, setCorrectionBook] = useState("");
  const [correctionChapter, setCorrectionChapter] = useState("");
  const [historyOpen, setHistoryOpen] = useState(false);
  const [history, setHistory] = useState<ServiceSession[]>([]);
  const [historyDetail, setHistoryDetail] = useState<{ service: ServiceSession; timeline: TimelineEntry[] } | null>(
    null,
  );
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  const refreshStatus = useCallback(() => {
    commands.getLiveStatus().then(setStatus).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refreshStatus();
    const interval = window.setInterval(refreshStatus, STATUS_POLL_MS);
    return () => window.clearInterval(interval);
  }, [refreshStatus]);

  useEffect(() => {
    commands.listAudioDevices().then(setDevices).catch(() => {});
  }, [status?.audioStatus]);

  // Enabled Bible translations (Phase 1.5) - populates the search
  // translation selector. Disabled content is already excluded by the
  // backend (see `list_bible_translations`), so every entry here is
  // selectable.
  useEffect(() => {
    commands.listBibleTranslations().then(setTranslations).catch(() => {});
  }, []);

  const refreshContentRegistry = useCallback(() => {
    commands.listContentRegistry().then(setContentItems).catch(() => {});
    commands.listBibleTranslations().then(setTranslations).catch(() => {});
  }, []);

  useEffect(() => {
    refreshContentRegistry();
  }, [refreshContentRegistry]);

  useEffect(() => {
    commands.getIntelligenceCapabilities().then(setIntelligenceCapabilities).catch(() => {});
  }, []);

  const activeServiceId = status?.service?.id;
  useEffect(() => {
    if (activeServiceId) {
      commands.listSuggestions("pending").then(setSuggestions).catch(() => {});
      commands.listSuggestions("approved").then(setApprovedSuggestions).catch(() => {});
      commands.listPreparedPresentations().then(setPreparedItems).catch(() => {});
      commands.listTranscript(TRANSCRIPT_LIMIT).then(setTranscript).catch(() => {});
      commands.listTimeline(TIMELINE_LIMIT).then(setTimeline).catch(() => {});
      commands.listMusicFindings().then(setMusicFindings).catch(() => {});
      commands.listSermonFindings().then(setSermonFindings).catch(() => {});
      commands.getSermonState().then(setSermonState).catch(() => {});
    } else {
      setSuggestions([]);
      setApprovedSuggestions([]);
      setPreparedItems([]);
      setPreviews({});
      setSearchPreviews({});
      setTranscript([]);
      setTranscriptReceivedAt({});
      setTimeline([]);
      setActiveContext(null);
      setLastReference(null);
      setRecentReferences([]);
      setAmbiguous([]);
      setMusicFindings([]);
      setSermonFindings([]);
      setSermonState(null);
    }
  }, [activeServiceId]);

  // Timeline is periodically refreshed alongside status - it's a
  // secondary/history view, not something every single event needs to
  // push into individually.
  useEffect(() => {
    if (!activeServiceId) return;
    const interval = window.setInterval(() => {
      commands.listTimeline(TIMELINE_LIMIT).then(setTimeline).catch(() => {});
    }, STATUS_POLL_MS);
    return () => window.clearInterval(interval);
  }, [activeServiceId]);

  const recordDetection = useCallback((detection: ScriptureDetection) => {
    if (detection.kind === "ambiguous") {
      setAmbiguous((prev) => [detection, ...prev].slice(0, AMBIGUOUS_LIMIT));
      return;
    }
    if (detection.context) setActiveContext(detection.context);
    if (detection.reference) {
      const reference = detection.reference;
      setLastReference(reference);
      setRecentReferences((prev) => {
        const display = referenceDisplay(reference);
        if (prev[0] && referenceDisplay(prev[0]) === display) return prev;
        return [reference, ...prev].slice(0, RECENT_REFERENCES_LIMIT);
      });
    }
  }, []);

  useEffect(() => {
    const subscriptions = [
      liveEvents.onTranscriptUpdated((segment) => {
        if (!segment.isFinal) return; // interim text is not added to the permanent feed
        setTranscript((prev) => [...prev.slice(-(TRANSCRIPT_LIMIT - 1)), segment]);
        setTranscriptReceivedAt((prev) => ({ ...prev, [segment.id]: new Date().toISOString() }));
      }),
      liveEvents.onScriptureDetected(recordDetection),
      liveEvents.onScriptureUpdated(recordDetection),
      liveEvents.onSuggestionCreated((s) => setSuggestions((prev) => [s, ...prev])),
      liveEvents.onSuggestionApproved((s) => {
        setSuggestions((prev) => prev.filter((x) => x.id !== s.id));
        setApprovedSuggestions((prev) => [s, ...prev]);
      }),
      liveEvents.onSuggestionRejected((s) => setSuggestions((prev) => prev.filter((x) => x.id !== s.id))),
      liveEvents.onSuggestionEdited((s) => setSuggestions((prev) => prev.map((x) => (x.id === s.id ? s : x)))),
      liveEvents.onPresentationPrepared((item) => {
        setPreparedItems((prev) => [item, ...prev]);
        if (item.sourceSuggestionId) {
          setApprovedSuggestions((prev) => prev.filter((s) => s.id !== item.sourceSuggestionId));
        }
      }),
      liveEvents.onPresentationCancelled((item) =>
        setPreparedItems((prev) => prev.filter((x) => x.id !== item.id)),
      ),
      liveEvents.onMusicFindingDetected((finding) => setMusicFindings((prev) => [finding, ...prev])),
      liveEvents.onMusicFindingAccepted((finding) =>
        setMusicFindings((prev) => prev.filter((f) => f.id !== finding.id)),
      ),
      liveEvents.onMusicFindingRejected((finding) =>
        setMusicFindings((prev) => prev.filter((f) => f.id !== finding.id)),
      ),
      liveEvents.onCurrentSongChanged((song) =>
        setStatus((prev) => (prev ? { ...prev, currentSong: song } : prev)),
      ),
      liveEvents.onSermonFindingDetected((finding) => setSermonFindings((prev) => [finding, ...prev])),
      liveEvents.onSermonFindingAccepted((finding) =>
        setSermonFindings((prev) => prev.filter((f) => f.id !== finding.id)),
      ),
      liveEvents.onSermonFindingRejected((finding) =>
        setSermonFindings((prev) => prev.filter((f) => f.id !== finding.id)),
      ),
      liveEvents.onSermonStateChanged((state) =>
        setSermonState((prev) => (prev ? { ...prev, state } : { state, theme: null, points: [] })),
      ),
      liveEvents.onSermonThemeChanged((theme) =>
        setSermonState((prev) => (prev ? { ...prev, theme } : prev)),
      ),
      liveEvents.onSermonStructureUpdated((points) =>
        setSermonState((prev) => (prev ? { ...prev, points } : prev)),
      ),
    ];
    return () => {
      subscriptions.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [recordDetection]);

  const withBusy = useCallback(async (key: string, action: () => Promise<void>) => {
    setBusy(key);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }, []);

  const isBusy = (key: string) => busy === key;

  const firstPending = suggestions[0];

  // Phase 1.3 keyboard shortcuts (section 31): A/R/E act on the first
  // pending suggestion, S focuses the search box. P previews it (Phase
  // 1.4) - never prepares, since a still-pending suggestion is never
  // approved yet and preparing requires approval (section 3/16). Guarded
  // so typing in any input never accidentally triggers one - see
  // `shouldHandleShortcut`.
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const eventLike = {
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
        altKey: event.altKey,
        target: target ? { tagName: target.tagName, isContentEditable: target.isContentEditable } : null,
      };
      if (!shouldHandleShortcut(eventLike)) return;
      const key = event.key.toLowerCase();
      if (key === "s") {
        event.preventDefault();
        searchInputRef.current?.focus();
        return;
      }
      if (!firstPending || busy === `approve-${firstPending.id}` || busy === `reject-${firstPending.id}`) return;
      if (key === "a") {
        void withBusy(`approve-${firstPending.id}`, async () => {
          await commands.approveSuggestion(firstPending.id);
        });
      } else if (key === "r") {
        void withBusy(`reject-${firstPending.id}`, async () => {
          await commands.rejectSuggestion(firstPending.id);
        });
      } else if (key === "e") {
        setEditingId(firstPending.id);
        setEditValue(firstPending.kind.type === "scripture" ? firstPending.kind.reference : "");
      } else if (key === "p") {
        void withBusy(`preview-${firstPending.id}`, async () => {
          const preview = await commands.previewPresentation(firstPending.id);
          setPreviews((prev) => ({ ...prev, [firstPending.id]: preview }));
        });
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [firstPending, withBusy, busy]);

  const openHistory = () => {
    setHistoryOpen((open) => !open);
    if (!historyOpen && history.length === 0) {
      commands.listServiceHistory(20).then(setHistory).catch((e) => setError(String(e)));
    }
  };

  // Opening a past service's detail is read-only and entirely separate
  // state from the live view above - it must never overwrite
  // activeContext/lastReference/suggestions (section 20).
  const viewHistoryService = (service: ServiceSession) => {
    commands
      .listTimeline(TIMELINE_LIMIT, service.id)
      .then((entries) => setHistoryDetail({ service, timeline: entries }))
      .catch((e) => setError(String(e)));
  };

  return (
    <div className="live-brain">
      <header className="live-brain__header">
        <h1>CIP &mdash; Live Service</h1>
        {error && (
          <p className="live-brain__error" role="alert">
            {error}
          </p>
        )}
      </header>

      <StatusBar status={status} />

      <section className="live-brain__panel">
        <h2>Service</h2>
        {!status?.service || status.serviceStatus === "completed" ? (
          <div className="live-brain__row">
            <input
              value={serviceTitle}
              onChange={(e) => setServiceTitle(e.target.value)}
              placeholder="Service title"
              aria-label="Service title"
            />
            <button
              type="button"
              disabled={isBusy("start-service")}
              onClick={() =>
                withBusy("start-service", async () => {
                  await commands.startService(serviceTitle);
                  refreshStatus();
                })
              }
            >
              Start Service
            </button>
          </div>
        ) : (
          <div className="live-brain__row">
            <span>
              {status.service.title} &mdash; <strong>{status.serviceStatus.toUpperCase()}</strong>
            </span>
            {status.serviceStatus === "live" && (
              <button
                type="button"
                disabled={isBusy("pause-service")}
                onClick={() =>
                  withBusy("pause-service", async () => {
                    await commands.pauseService();
                    refreshStatus();
                  })
                }
              >
                Pause
              </button>
            )}
            {status.serviceStatus === "paused" && (
              <button
                type="button"
                disabled={isBusy("resume-service")}
                onClick={() =>
                  withBusy("resume-service", async () => {
                    await commands.resumeService();
                    refreshStatus();
                  })
                }
              >
                Resume
              </button>
            )}
            <button
              type="button"
              disabled={isBusy("end-service")}
              onClick={() =>
                withBusy("end-service", async () => {
                  await commands.endService();
                  refreshStatus();
                })
              }
            >
              End Service
            </button>
          </div>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>Audio &amp; Speech</h2>
        {status?.speechStatus === "unavailable" && (
          <p className="live-brain__notice">
            SPEECH UNAVAILABLE &mdash; manual operation remains available (search, prepare, approve below).
          </p>
        )}
        {status?.audioStatus === "error" && (
          <p className="live-brain__notice">AUDIO ERROR &mdash; retry below. The service remains live.</p>
        )}
        {status?.speechStatus === "error" && (
          <p className="live-brain__notice">SPEECH ERROR &mdash; recorded, will clear on the next successful chunk.</p>
        )}
        {devices.length === 0 && <p className="live-brain__notice">NO_AUDIO_DEVICE &mdash; connect or select an audio input device.</p>}
        <div className="live-brain__row">
          <select value={selectedDevice} onChange={(e) => setSelectedDevice(e.target.value)} aria-label="Audio input device">
            <option value="">Default device</option>
            {devices.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
                {d.isDefault ? " (default)" : ""}
              </option>
            ))}
          </select>
          {status?.audioStatus === "listening" ? (
            <button
              type="button"
              disabled={isBusy("stop-listening")}
              onClick={() =>
                withBusy("stop-listening", async () => {
                  await commands.stopListening();
                  refreshStatus();
                })
              }
            >
              Stop Listening
            </button>
          ) : (
            <button
              type="button"
              disabled={!status?.service || status.speechStatus === "unavailable" || isBusy("start-listening")}
              onClick={() =>
                withBusy("start-listening", async () => {
                  await commands.startListening(selectedDevice || undefined);
                  refreshStatus();
                })
              }
            >
              {status?.audioStatus === "error" ? "Retry" : "Start Listening"}
            </button>
          )}
        </div>

        <details className="live-brain__manual-entry">
          <summary>Manual / test transcript entry</summary>
          <p className="live-brain__hint">
            Feeds text through the same Bible Intelligence Core pipeline real speech would - useful for testing, or
            as a fallback while speech recognition is unavailable.
          </p>
          <div className="live-brain__row">
            <input
              value={manualText}
              onChange={(e) => setManualText(e.target.value)}
              placeholder='e.g. "Turn with me to Romans chapter 8."'
              aria-label="Manual transcript text"
            />
            <button
              type="button"
              disabled={!status?.service || !manualText.trim() || isBusy("manual-transcript")}
              onClick={() =>
                withBusy("manual-transcript", async () => {
                  await commands.processTestTranscript(manualText.trim());
                  setManualText("");
                })
              }
            >
              Submit
            </button>
          </div>
        </details>
      </section>

      <section className="live-brain__panel">
        <h2>Current Scripture</h2>
        <div className="live-brain__current-scripture">
          <div>
            <span className="live-brain__label">Current reference</span>
            <p className="live-brain__active-context">{lastReference ? referenceDisplay(lastReference) : "—"}</p>
          </div>
          <div>
            <span className="live-brain__label">Active context</span>
            <p>
              {activeContext ? (
                <>
                  {activeContext.book} {activeContext.chapter}
                  <span className="live-brain__confidence"> &mdash; confidence {activeContext.confidence.level}</span>
                </>
              ) : (
                "No active context"
              )}
            </p>
          </div>
        </div>

        <details className="live-brain__manual-entry">
          <summary>Correct active context</summary>
          <p className="live-brain__hint">If CIP misunderstood the pastor, set the correct book and chapter here.</p>
          <div className="live-brain__row">
            <input
              value={correctionBook}
              onChange={(e) => setCorrectionBook(e.target.value)}
              placeholder="e.g. ROM"
              aria-label="Correction book"
            />
            <input
              value={correctionChapter}
              onChange={(e) => setCorrectionChapter(e.target.value)}
              placeholder="e.g. 8"
              inputMode="numeric"
              aria-label="Correction chapter"
            />
            <button
              type="button"
              disabled={!status?.service || !correctionBook.trim() || !correctionChapter.trim() || isBusy("correct-context")}
              onClick={() =>
                withBusy("correct-context", async () => {
                  const chapter = Number.parseInt(correctionChapter, 10);
                  const corrected = await commands.correctScriptureContext(correctionBook.trim().toUpperCase(), chapter);
                  setActiveContext(corrected);
                  setCorrectionBook("");
                  setCorrectionChapter("");
                })
              }
            >
              Correct
            </button>
          </div>
        </details>

        {recentReferences.length > 0 && (
          <div className="live-brain__recent-references">
            <span className="live-brain__label">Recent references</span>
            <ul>
              {recentReferences.map((ref, i) => (
                <li key={`${referenceDisplay(ref)}-${i}`}>{referenceDisplay(ref)}</li>
              ))}
            </ul>
          </div>
        )}
      </section>

      {ambiguous.length > 0 && (
        <section className="live-brain__panel live-brain__panel--ambiguous">
          <h2>Ambiguous Scripture</h2>
          {ambiguous.map((detection, i) => (
            <AmbiguousCard
              key={`${detection.rawText}-${i}`}
              detection={detection}
              busy={busy}
              onSelect={(candidate) =>
                withBusy(`resolve-ambiguous-${i}`, async () => {
                  await commands.resolveAmbiguousReference(
                    candidate.reference.book,
                    candidate.reference.chapter,
                    candidate.reference.verseStart,
                    detection.rawText,
                    detection.candidates.map((c) => referenceDisplay(c.reference)),
                  );
                  setAmbiguous((prev) => prev.filter((_, index) => index !== i));
                })
              }
              onDismiss={() => setAmbiguous((prev) => prev.filter((_, index) => index !== i))}
            />
          ))}
        </section>
      )}

      <section className="live-brain__panel">
        <h2>Live Transcript</h2>
        {transcript.length === 0 ? (
          <p className="live-brain__hint">Nothing transcribed yet.</p>
        ) : (
          <ul className="live-brain__transcript">
            {transcript.map((segment) => (
              <li key={segment.id}>
                {transcriptReceivedAt[segment.id] && (
                  <span className="live-brain__timestamp">{formatClockTime(transcriptReceivedAt[segment.id])}</span>
                )}
                &ldquo;{segment.text}&rdquo;
                {segment.confidence && (
                  <span className="live-brain__confidence"> ({Math.round(segment.confidence.score * 100)}%)</span>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>
          Pending Suggestions <span className="live-brain__hint">(A/R/E/P act on the first one)</span>
        </h2>
        {suggestions.length === 0 ? (
          <p className="live-brain__hint">No pending suggestions.</p>
        ) : (
          <SuggestionGroups
            suggestions={suggestions}
            busy={busy}
            editingId={editingId}
            editValue={editValue}
            onEditValueChange={setEditValue}
            onStartEdit={(id, currentReference) => {
              setEditingId(id);
              setEditValue(currentReference);
            }}
            onCancelEdit={() => setEditingId(null)}
            onApprove={(id) =>
              withBusy(`approve-${id}`, async () => {
                await commands.approveSuggestion(id);
              })
            }
            onSaveEdit={(id) =>
              withBusy(`edit-${id}`, async () => {
                await commands.editSuggestion(id, editValue.trim());
                setEditingId(null);
              })
            }
            onReject={(id) =>
              withBusy(`reject-${id}`, async () => {
                await commands.rejectSuggestion(id);
              })
            }
            onPreview={(id) =>
              withBusy(`preview-${id}`, async () => {
                const preview = await commands.previewPresentation(id);
                setPreviews((prev) => ({ ...prev, [id]: preview }));
              })
            }
            previews={previews}
          />
        )}
      </section>

      {approvedSuggestions.length > 0 && (
        <section className="live-brain__panel">
          <h2>Approved &mdash; Ready to Prepare</h2>
          <p className="live-brain__hint">
            Approved suggestions never auto-prepare - preview, then explicitly prepare each one.
          </p>
          <ul className="live-brain__suggestions">
            {approvedSuggestions.map((s) => (
              <ApprovedSuggestionCard
                key={s.id}
                suggestion={s}
                busy={busy}
                preview={previews[s.id]}
                onPreview={() =>
                  withBusy(`preview-${s.id}`, async () => {
                    const preview = await commands.previewPresentation(s.id);
                    setPreviews((prev) => ({ ...prev, [s.id]: preview }));
                  })
                }
                onPrepare={() =>
                  withBusy(`prepare-${s.id}`, async () => {
                    await commands.preparePresentation(s.id);
                  })
                }
              />
            ))}
          </ul>
        </section>
      )}

      <section className="live-brain__panel">
        <h2>Service Timeline</h2>
        {timeline.length === 0 ? (
          <p className="live-brain__hint">No timeline events yet.</p>
        ) : (
          <ul className="live-brain__timeline">
            {timeline.map((entry) => (
              <li key={entry.id}>
                <span className="live-brain__timestamp">{formatClockTime(entry.createdAt)}</span>
                {describeTimelineEntry(entry)}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>Manual Bible Search</h2>
        <p className="live-brain__hint">Works with no speech model, no audio device, and no internet connection.</p>
        <div className="live-brain__row">
          <input
            ref={searchInputRef}
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="e.g. Romans 8:28, Romans 8, or a phrase"
            aria-label="Bible search query"
          />
          <select
            value={searchTranslation}
            onChange={(e) => setSearchTranslation(e.target.value)}
            aria-label="Search translation"
          >
            {translations.length === 0 ? (
              <option value="">Default translation</option>
            ) : (
              translations.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))
            )}
          </select>
          <button
            type="button"
            disabled={!searchQuery.trim() || isBusy("search")}
            onClick={() =>
              withBusy("search", async () => {
                const results = await commands.searchBible(searchQuery.trim(), searchTranslation || undefined);
                setSearchResults(results);
              })
            }
          >
            Search
          </button>
        </div>
        {searchResults.length > 0 && (
          <ul className="live-brain__search-results">
            {searchResults.map((result) => (
              <li key={`${result.translationId}-${result.reference}`}>
                <strong>{result.reference}</strong>
                <span className="live-brain__confidence"> ({result.translationId})</span>
                <span> &mdash; {result.text}</span>
                <div className="live-brain__row">
                  <button
                    type="button"
                    disabled={isBusy(`search-preview-${result.reference}`)}
                    onClick={() =>
                      withBusy(`search-preview-${result.reference}`, async () => {
                        const preview = await commands.previewScripture(result.reference);
                        setSearchPreviews((prev) => ({ ...prev, [result.reference]: preview }));
                      })
                    }
                  >
                    Preview
                  </button>
                  <button
                    type="button"
                    disabled={!status?.service || isBusy(`search-prepare-${result.reference}`)}
                    onClick={() =>
                      withBusy(`search-prepare-${result.reference}`, async () => {
                        await commands.createManualPresentation(result.reference);
                      })
                    }
                  >
                    Prepare
                  </button>
                </div>
                {searchPreviews[result.reference] && <PreviewPane preview={searchPreviews[result.reference]} />}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="live-brain__panel live-brain__panel--current-output">
        <h2>Current Output</h2>
        {preparedItems.length === 0 ? (
          <p className="live-brain__hint">NOTHING PREPARED &mdash; CIP never prepares or projects content automatically.</p>
        ) : (
          <ul className="live-brain__prepared-items">
            {preparedItems.map((item) => (
              <li key={item.id}>
                <div className="live-brain__suggestion-header">
                  <strong>{item.content.type === "scripture" ? item.content.reference : item.content.title}</strong>
                  <span className="live-brain__status-dot">
                    &#9679; PREPARED{item.sourceSuggestionId ? " (automatic)" : " (manual)"}
                  </span>
                </div>
                {item.content.type === "scripture" && (
                  <p className="live-brain__preview-heading">
                    {item.content.text} <span className="live-brain__confidence">({item.content.translationId})</span>
                  </p>
                )}
                <div className="live-brain__row">
                  <button
                    type="button"
                    disabled={isBusy(`cancel-${item.id}`)}
                    onClick={() =>
                      withBusy(`cancel-${item.id}`, async () => {
                        await commands.cancelPresentation(item.id);
                      })
                    }
                  >
                    Cancel
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>
          Content Registry{" "}
          <button type="button" onClick={refreshContentRegistry}>
            Refresh
          </button>
        </h2>
        <p className="live-brain__hint">
          Installed local content. Copyrighted Bible datasets are never downloaded automatically - only content the
          user explicitly imports, with honestly-recorded (or UNKNOWN) licensing.
        </p>
        {contentItems.length === 0 ? (
          <p className="live-brain__hint">No content registered.</p>
        ) : (
          <ul className="live-brain__content-items">
            {contentItems.map((item) => {
              const translationId = item.id.startsWith("bible:") ? item.id.slice("bible:".length) : null;
              const integrity = translationId ? integrityByTranslation[translationId] : undefined;
              return (
                <li key={item.id}>
                  <div className="live-brain__suggestion-header">
                    <strong>{item.name}</strong>
                    <span className="live-brain__confidence">
                      {item.status === "enabled" ? "● ENABLED" : "○ DISABLED"}
                    </span>
                  </div>
                  <p className="live-brain__hint">
                    {item.contentType} &middot; {item.language} &middot; version {item.version} &middot; publisher:{" "}
                    {item.publisher ?? "UNKNOWN"} &middot; license: {item.license ?? "UNKNOWN"} &middot; distribution:{" "}
                    {item.distribution ?? "UNKNOWN"}
                  </p>
                  {integrity && (
                    <p className="live-brain__hint">
                      Integrity: {integrity.status.toUpperCase()} ({integrity.booksPresent}/{integrity.booksExpected}{" "}
                      books, {integrity.versesChecked} verses checked
                      {integrity.issues.length > 0 ? `, ${integrity.issues.length} issue(s)` : ""})
                    </p>
                  )}
                  <div className="live-brain__row">
                    <button
                      type="button"
                      disabled={isBusy(`content-toggle-${item.id}`)}
                      onClick={() =>
                        withBusy(`content-toggle-${item.id}`, async () => {
                          await commands.setContentEnabled(item.id, item.status !== "enabled");
                          refreshContentRegistry();
                        })
                      }
                    >
                      {item.status === "enabled" ? "Disable" : "Enable"}
                    </button>
                    {translationId && (
                      <button
                        type="button"
                        disabled={isBusy(`content-integrity-${item.id}`)}
                        onClick={() =>
                          withBusy(`content-integrity-${item.id}`, async () => {
                            const report = await commands.checkBibleDatasetIntegrity(translationId);
                            setIntegrityByTranslation((prev) => ({ ...prev, [translationId]: report }));
                          })
                        }
                      >
                        Check Integrity
                      </button>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}

        <details className="live-brain__manual-entry">
          <summary>Import a Bible dataset</summary>
          <p className="live-brain__hint">
            Select a local JSON file (see docs/bible-datasets.md for the format). The file is read in the browser -
            nothing is downloaded, and the backend never touches the filesystem itself.
          </p>
          <div className="live-brain__row">
            <input
              type="file"
              accept="application/json"
              aria-label="Bible dataset file"
              disabled={isBusy("import-dataset")}
              onChange={(e) => {
                const file = e.target.files?.[0];
                e.target.value = "";
                if (!file) return;
                const reader = new FileReader();
                reader.onload = () => {
                  const text = typeof reader.result === "string" ? reader.result : "";
                  void withBusy("import-dataset", async () => {
                    const report = await commands.importBibleDataset(text);
                    setImportResult(report);
                    setImportFileName(file.name);
                    refreshContentRegistry();
                  });
                };
                reader.onerror = () => setError(`Failed to read ${file.name}`);
                reader.readAsText(file);
              }}
            />
          </div>
          {importResult && (
            <div className="live-brain__preview-pane">
              <p className="live-brain__label">
                Import result{importFileName ? ` — ${importFileName}` : ""}
              </p>
              <p>
                {importResult.translationId} (dataset version {importResult.datasetVersion}) &mdash; {importResult.books}{" "}
                book(s), {importResult.chapters} chapter(s), {importResult.versesTotal} verse(s) in file
              </p>
              <p>
                Imported: {importResult.imported} &middot; Already present: {importResult.alreadyPresent} &middot;
                Invalid: {importResult.invalid}
              </p>
              <p className="live-brain__confidence">Checksum: {importResult.checksum}</p>
              {importResult.errors.length > 0 && (
                <ul>
                  {importResult.errors.map((message, i) => (
                    <li key={i}>{message}</li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </details>
      </section>

      <section className="live-brain__panel">
        <h2>Intelligence Status</h2>
        <p className="live-brain__hint">
          The Phase 2.0 shared intelligence architecture. Only Bible has a real engine behind it - the rest reserve
          the shape future phases will fill in, and are honestly reported as not installed rather than faked.
        </p>
        <ul className="live-brain__content-items">
          {intelligenceCapabilities.map((report) => (
            <li key={report.domain}>
              <div className="live-brain__suggestion-header">
                <strong>{report.domain}</strong>
                <span className="live-brain__confidence">{report.capability.toUpperCase()}</span>
              </div>
              {report.engineId && (
                <p className="live-brain__hint">
                  {report.engineId} v{report.engineVersion}
                </p>
              )}
            </li>
          ))}
        </ul>
      </section>

      <section className="live-brain__panel">
        <h2>Music Intelligence</h2>
        <p className="live-brain__hint">
          Deterministic title/alias/number/lyric recognition (Phase 2.1), plus acoustic (audio-fingerprint)
          recognition (Phase 2.2) when a local model is configured. Findings are never automatically prepared as a
          presentation item; accept or reject each one explicitly.
        </p>

        {status?.acousticStatus && (
          <p className="live-brain__hint">
            Acoustic recognition: <strong>{status.acousticStatus.status}</strong>
            {status.acousticStatus.method !== "none" && <> ({status.acousticStatus.method})</>}
            {status.acousticStatus.reason && <> &mdash; {status.acousticStatus.reason}</>}
          </p>
        )}

        <div className="live-brain__suggestion-card">
          {status?.currentSong ? (
            <>
              <div className="live-brain__suggestion-header">
                <strong>
                  Current song: {status.currentSong.songId} ({status.currentSong.contentId})
                </strong>
                <span className="live-brain__confidence">
                  {Math.round(status.currentSong.confidence.score * 100)}%
                </span>
              </div>
              <button
                type="button"
                disabled={isBusy("clear-current-song")}
                onClick={() => withBusy("clear-current-song", () => commands.clearCurrentSong())}
              >
                Clear current song
              </button>
            </>
          ) : (
            <p className="live-brain__hint">
              No current song - accept a finding below to confirm one. A pending finding is only ever a candidate,
              never automatically current.
            </p>
          )}
        </div>

        <details className="live-brain__manual-entry">
          <summary>Manual / test music transcript entry</summary>
          <p className="live-brain__hint">
            Feeds text through the Music Intelligence engine the same way a real transcript segment would.
          </p>
          <div className="live-brain__row">
            <input
              value={musicManualText}
              onChange={(e) => setMusicManualText(e.target.value)}
              placeholder='e.g. "Let&rsquo;s sing Amazing Grace" or a line of lyrics'
              aria-label="Manual music transcript text"
            />
            <button
              type="button"
              disabled={!status?.service || !musicManualText.trim() || isBusy("music-transcript")}
              onClick={() =>
                withBusy("music-transcript", async () => {
                  await commands.analyzeMusicTranscript(musicManualText.trim());
                  setMusicManualText("");
                })
              }
            >
              Submit
            </button>
          </div>
        </details>

        {musicFindings.length === 0 ? (
          <p className="live-brain__hint">No pending music findings.</p>
        ) : (
          <ul className="live-brain__suggestions">
            {musicFindings.map((finding) => (
              <li key={finding.id} className="live-brain__suggestion-card">
                <div className="live-brain__suggestion-header">
                  <strong>{finding.summary}</strong>
                  <span className="live-brain__confidence">
                    Confidence: {Math.round(finding.confidence.score * 100)}%
                  </span>
                </div>
                <p className="live-brain__hint">
                  {finding.assertionLevel} &middot; {finding.provenance.contentId ?? "unknown dataset"}
                  {finding.evidence.some((e) => e.kind === "acoustic") && <> &middot; acoustic</>}
                </p>
                <div className="live-brain__row">
                  <button
                    type="button"
                    disabled={isBusy(`music-accept-${finding.id}`)}
                    onClick={() =>
                      withBusy(`music-accept-${finding.id}`, async () => {
                        await commands.acceptMusicFinding(finding.id);
                      })
                    }
                  >
                    Accept
                  </button>
                  <button
                    type="button"
                    disabled={isBusy(`music-reject-${finding.id}`)}
                    onClick={() =>
                      withBusy(`music-reject-${finding.id}`, async () => {
                        await commands.rejectMusicFinding(finding.id);
                      })
                    }
                  >
                    Reject
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}

        <details className="live-brain__manual-entry">
          <summary>Song search</summary>
          <p className="live-brain__hint">
            Ad hoc title/number/lyric lookup across currently-enabled Music datasets - works with no active service.
          </p>
          <div className="live-brain__row">
            <input
              value={musicSearchQuery}
              onChange={(e) => setMusicSearchQuery(e.target.value)}
              placeholder="Search text"
              aria-label="Music search query"
            />
            <select
              value={musicSearchType}
              onChange={(e) => setMusicSearchType(e.target.value as MusicQueryType)}
              aria-label="Music search type"
            >
              <option value="title">Title/alias</option>
              <option value="number">Number</option>
              <option value="lyric">Lyric</option>
            </select>
            <button
              type="button"
              disabled={!musicSearchQuery.trim() || isBusy("music-search")}
              onClick={() =>
                withBusy("music-search", async () => {
                  const results = await commands.searchMusic(musicSearchQuery.trim(), musicSearchType);
                  setMusicSearchResults(results);
                })
              }
            >
              Search
            </button>
          </div>
          {musicSearchResults && (
            <ul className="live-brain__content-items">
              {musicSearchResults.length === 0 ? (
                <li className="live-brain__hint">No matches.</li>
              ) : (
                musicSearchResults.map((candidate) => (
                  <li key={`${candidate.source}:${candidate.songId}`}>
                    <div className="live-brain__suggestion-header">
                      <strong>{candidate.explanation}</strong>
                      <span className="live-brain__confidence">
                        {Math.round(candidate.confidence.score * 100)}%
                      </span>
                    </div>
                    <p className="live-brain__hint">
                      {candidate.source} &middot; {candidate.matchType}
                    </p>
                  </li>
                ))
              )}
            </ul>
          )}
        </details>
      </section>

      <section className="live-brain__panel">
        <h2>Sermon Intelligence</h2>
        <p className="live-brain__hint">
          Deterministic structural/theme detection from live or manually-entered transcript text (Phase 2.3).
          Every item below is clearly marked Observed or Inferred - CIP never claims the pastor said something that
          was not directly detected in the transcript, and nothing here is ever automatically presented.
        </p>

        <div className="live-brain__suggestion-card">
          <div className="live-brain__suggestion-header">
            <strong>Current state: {sermonState?.state ?? "introduction"}</strong>
          </div>
          {sermonState?.theme ? (
            <p className="live-brain__hint">
              Current theme (Inferred): <strong>{sermonState.theme.label}</strong>{" "}
              ({Math.round(sermonState.theme.confidence * 100)}% - {sermonState.theme.repetitionCount} repetition(s),{" "}
              {sermonState.theme.structuralMentions} structural mention(s))
            </p>
          ) : (
            <p className="live-brain__hint">No theme candidate yet - needs more repeated, structurally-evidenced content.</p>
          )}
          {sermonState && sermonState.points.length > 0 ? (
            <>
              <p className="live-brain__hint">
                Current main point: <strong>{sermonState.points[sermonState.points.length - 1].rawText}</strong>
              </p>
              <details className="live-brain__manual-entry">
                <summary>Recent points ({sermonState.points.length})</summary>
                <ul className="live-brain__content-items">
                  {sermonState.points.map((point) => (
                    <li key={point.sequence}>
                      <strong>
                        Point {point.sequence}: {point.rawText}
                      </strong>
                      {point.subPoints.length > 0 && (
                        <ul>
                          {point.subPoints.map((sub) => (
                            <li key={sub.sequence}>{sub.rawText}</li>
                          ))}
                        </ul>
                      )}
                    </li>
                  ))}
                </ul>
              </details>
            </>
          ) : (
            <p className="live-brain__hint">No main point recorded yet.</p>
          )}
        </div>

        <details className="live-brain__manual-entry">
          <summary>Manual / test sermon transcript entry</summary>
          <p className="live-brain__hint">
            Feeds text through the same `SermonIntelligenceEngine` real transcript would use - deterministic testing
            without audio hardware (spec section 48).
          </p>
          <div className="live-brain__row">
            <input
              value={sermonManualText}
              onChange={(e) => setSermonManualText(e.target.value)}
              placeholder='e.g. "My first point is that faith comes by hearing."'
              aria-label="Manual sermon transcript text"
            />
            <button
              type="button"
              disabled={!status?.service || !sermonManualText.trim() || isBusy("sermon-transcript")}
              onClick={() =>
                withBusy("sermon-transcript", async () => {
                  await commands.analyzeSermonTranscript(sermonManualText.trim());
                  setSermonManualText("");
                  commands.getSermonState().then(setSermonState).catch(() => {});
                })
              }
            >
              Submit
            </button>
          </div>
        </details>

        {sermonFindings.length === 0 ? (
          <p className="live-brain__hint">No pending sermon findings.</p>
        ) : (
          <ul className="live-brain__suggestions">
            {sermonFindings.map((finding) => (
              <li key={finding.id} className="live-brain__suggestion-card">
                <div className="live-brain__suggestion-header">
                  <strong>{finding.summary}</strong>
                  <span className="live-brain__confidence">
                    Confidence: {Math.round(finding.confidence.score * 100)}%
                  </span>
                </div>
                <p className="live-brain__hint">
                  {finding.assertionLevel.toUpperCase()}
                  {finding.transcriptSegmentIds.length > 0 && (
                    <> &middot; source segment(s): {finding.transcriptSegmentIds.length}</>
                  )}
                </p>
                <div className="live-brain__row">
                  <button
                    type="button"
                    disabled={isBusy(`sermon-accept-${finding.id}`)}
                    onClick={() =>
                      withBusy(`sermon-accept-${finding.id}`, async () => {
                        await commands.acceptSermonFinding(finding.id);
                      })
                    }
                  >
                    Accept
                  </button>
                  <button
                    type="button"
                    disabled={isBusy(`sermon-reject-${finding.id}`)}
                    onClick={() =>
                      withBusy(`sermon-reject-${finding.id}`, async () => {
                        await commands.rejectSermonFinding(finding.id);
                      })
                    }
                  >
                    Reject
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>
          Service History{" "}
          <button type="button" onClick={openHistory}>
            {historyOpen ? "Hide" : "Show"}
          </button>
        </h2>
        {historyOpen && (
          <>
            {history.length === 0 ? (
              <p className="live-brain__hint">No completed services yet.</p>
            ) : (
              <ul className="live-brain__history-list">
                {history.map((service) => (
                  <li key={service.id}>
                    <button type="button" onClick={() => viewHistoryService(service)}>
                      {service.title} &mdash; {formatClockTime(service.startedAt)}
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {historyDetail && (
              <div className="live-brain__history-detail">
                <h3>{historyDetail.service.title}</h3>
                <p className="live-brain__hint">
                  Started {formatClockTime(historyDetail.service.startedAt)}
                  {historyDetail.service.endedAt ? ` — ended ${formatClockTime(historyDetail.service.endedAt)}` : ""}
                </p>
                <ul className="live-brain__timeline">
                  {historyDetail.timeline.map((entry) => (
                    <li key={entry.id}>
                      <span className="live-brain__timestamp">{formatClockTime(entry.createdAt)}</span>
                      {describeTimelineEntry(entry)}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </>
        )}
      </section>
    </div>
  );
}

function StatusBar({ status }: { status: LiveStatus | null }) {
  if (!status) return <p className="live-brain__hint">Connecting to backend&hellip;</p>;
  return (
    <div className="live-brain__status-bar">
      <StatusBadge label="Runtime" value="tauri" good={["tauri"]} />
      <StatusBadge label="Network" value={status.networkStatus} good={["online"]} />
      <StatusBadge label="Audio" value={status.audioStatus} good={["ready", "listening"]} />
      <StatusBadge label="Speech" value={status.speechStatus} good={["ready"]} />
      <StatusBadge label="AI" value={status.aiStatus} good={["available"]} />
      <StatusBadge label="Database" value={status.databaseStatus} good={["connected"]} />
    </div>
  );
}

function StatusBadge({ label, value, good }: { label: string; value: string; good: string[] }) {
  const isGood = good.includes(value);
  return (
    <span className={`live-brain__badge ${isGood ? "live-brain__badge--good" : "live-brain__badge--neutral"}`}>
      <span className="live-brain__dot" aria-hidden="true" />
      {label}: {value}
    </span>
  );
}

function AmbiguousCard({
  detection,
  busy,
  onSelect,
  onDismiss,
}: {
  detection: ScriptureDetection;
  busy: string | null;
  onSelect: (candidate: ScriptureDetection["candidates"][number]) => void;
  onDismiss: () => void;
}) {
  return (
    <div className="live-brain__ambiguous-card">
      <p className="live-brain__hint">Source: &ldquo;{detection.rawText}&rdquo;</p>
      <div className="live-brain__row">
        {detection.candidates.map((candidate) => (
          <button
            key={referenceDisplay(candidate.reference)}
            type="button"
            disabled={busy !== null}
            onClick={() => onSelect(candidate)}
          >
            Select {referenceDisplay(candidate.reference)} ({Math.round(candidate.confidence.score * 100)}%)
          </button>
        ))}
        <button type="button" onClick={onDismiss}>
          Dismiss
        </button>
      </div>
    </div>
  );
}

/** Renders a non-mutating [`PresentationPreview`] - the same real,
 * `BibleProvider`-sourced text and deterministic template a prepared item
 * would use, without persisting anything (section 14). */
function PreviewPane({ preview }: { preview: PresentationPreview }) {
  return (
    <div className="live-brain__preview-pane">
      <p className="live-brain__label">Preview &mdash; {preview.slide.template}</p>
      <p className="live-brain__preview-heading">
        <strong>{preview.slide.heading}</strong>
      </p>
      {preview.slide.bodyLines.map((line, i) => (
        <p key={i}>{line}</p>
      ))}
      {preview.slide.footer && <p className="live-brain__confidence">{preview.slide.footer}</p>}
    </div>
  );
}

interface SuggestionGroupsProps {
  suggestions: Suggestion[];
  busy: string | null;
  editingId: string | null;
  editValue: string;
  onEditValueChange: (value: string) => void;
  onStartEdit: (id: string, currentReference: string) => void;
  onCancelEdit: () => void;
  onApprove: (id: string) => void;
  onSaveEdit: (id: string) => void;
  onReject: (id: string) => void;
  onPreview: (id: string) => void;
  previews: Record<string, PresentationPreview>;
}

/** Groups pending suggestions by confidence level (section 14) - high
 * confidence first, so the operator's attention goes to the suggestions
 * most likely worth a quick approve. */
function SuggestionGroups(props: SuggestionGroupsProps) {
  const { suggestions } = props;
  const groups: Array<{ label: string; level: "high" | "medium" | "low" }> = [
    { label: "High Confidence", level: "high" },
    { label: "Medium Confidence", level: "medium" },
    { label: "Low Confidence", level: "low" },
  ];
  return (
    <>
      {groups.map(({ label, level }) => {
        const items = suggestions.filter((s) => s.confidence.level === level);
        if (items.length === 0) return null;
        return (
          <div key={level} className="live-brain__confidence-group">
            <h3>{label}</h3>
            <ul className="live-brain__suggestions">
              {items.map((s) => (
                <SuggestionCard key={s.id} suggestion={s} {...props} />
              ))}
            </ul>
          </div>
        );
      })}
    </>
  );
}

function SuggestionCard({
  suggestion,
  busy,
  editingId,
  editValue,
  onEditValueChange,
  onStartEdit,
  onCancelEdit,
  onApprove,
  onSaveEdit,
  onReject,
  onPreview,
  previews,
}: { suggestion: Suggestion } & Omit<SuggestionGroupsProps, "suggestions">) {
  const reference = suggestion.kind.type === "scripture" ? suggestion.kind.reference : suggestion.kind.label;
  const isEditing = editingId === suggestion.id;
  const confidencePercent = Math.round(suggestion.confidence.score * 100);
  const preview = previews[suggestion.id];

  return (
    <li className="live-brain__suggestion-card">
      {isEditing ? (
        <div className="live-brain__row">
          <input value={editValue} onChange={(e) => onEditValueChange(e.target.value)} aria-label="Edit reference" />
          <button type="button" disabled={busy === `edit-${suggestion.id}`} onClick={() => onSaveEdit(suggestion.id)}>
            Save
          </button>
          <button type="button" onClick={onCancelEdit}>
            Cancel
          </button>
        </div>
      ) : (
        <>
          <div className="live-brain__suggestion-header">
            <strong>{reference}</strong>
            <span className="live-brain__confidence">Confidence: {confidencePercent}%</span>
          </div>
          {suggestion.sourceText && <p className="live-brain__source-text">&ldquo;{suggestion.sourceText}&rdquo;</p>}
          <div className="live-brain__row">
            <button
              type="button"
              disabled={busy === `preview-${suggestion.id}`}
              onClick={() => onPreview(suggestion.id)}
            >
              Preview
            </button>
            <button type="button" disabled={busy === `approve-${suggestion.id}`} onClick={() => onApprove(suggestion.id)}>
              Approve
            </button>
            <button type="button" onClick={() => onStartEdit(suggestion.id, reference)}>
              Edit
            </button>
            <button type="button" disabled={busy === `reject-${suggestion.id}`} onClick={() => onReject(suggestion.id)}>
              Ignore
            </button>
          </div>
          {preview && <PreviewPane preview={preview} />}
        </>
      )}
    </li>
  );
}

/** An approved suggestion, not yet prepared (Phase 1.4) - "CURRENT
 * SUGGESTION -> after approval, PRESENTATION with PREVIEW/PREPARE" from
 * the workspace mockup. Never appears until the operator has explicitly
 * approved it; disappears once prepared (see the `PresentationPrepared`
 * event handler). */
function ApprovedSuggestionCard({
  suggestion,
  busy,
  preview,
  onPreview,
  onPrepare,
}: {
  suggestion: Suggestion;
  busy: string | null;
  preview: PresentationPreview | undefined;
  onPreview: () => void;
  onPrepare: () => void;
}) {
  const reference = suggestion.kind.type === "scripture" ? suggestion.kind.reference : suggestion.kind.label;
  return (
    <li className="live-brain__suggestion-card">
      <div className="live-brain__suggestion-header">
        <strong>{reference}</strong>
        <span className="live-brain__confidence">Approved</span>
      </div>
      <div className="live-brain__row">
        <button type="button" disabled={busy === `preview-${suggestion.id}`} onClick={onPreview}>
          Preview
        </button>
        <button type="button" disabled={busy === `prepare-${suggestion.id}`} onClick={onPrepare}>
          Prepare
        </button>
      </div>
      {preview && <PreviewPane preview={preview} />}
    </li>
  );
}
