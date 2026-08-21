import { useCallback, useEffect, useState } from "react";
import type { AudioDevice, BibleVerse, LiveStatus, ScriptureContext, Suggestion, TranscriptSegment } from "../domain";
import * as commands from "../lib/commands";
import * as liveEvents from "../lib/liveEvents";
import "./LiveChurchBrain.css";

const STATUS_POLL_MS = 3000;
const TRANSCRIPT_LIMIT = 20;

/**
 * Live Church Brain v0.1 - functional, not visually elaborate (per Phase
 * 1.2 scope). Deliberately keeps transcript / active context / detected
 * suggestion / approved content / projected content visually distinct -
 * "Do not merge these concepts." There is no projected-content control
 * anywhere here: preparing a presentation item is as far as this phase
 * goes (see `docs/live-speech.md`).
 */
export function LiveChurchBrain() {
  const [status, setStatus] = useState<LiveStatus | null>(null);
  const [activeContext, setActiveContext] = useState<ScriptureContext | null>(null);
  const [transcript, setTranscript] = useState<TranscriptSegment[]>([]);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [serviceTitle, setServiceTitle] = useState("Sunday Morning Service");
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [selectedDevice, setSelectedDevice] = useState("");
  const [manualText, setManualText] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<BibleVerse[]>([]);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

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

  const activeServiceId = status?.service?.id;
  useEffect(() => {
    if (activeServiceId) {
      commands.listSuggestions("pending").then(setSuggestions).catch(() => {});
      commands.listTranscript(TRANSCRIPT_LIMIT).then(setTranscript).catch(() => {});
    } else {
      setSuggestions([]);
      setTranscript([]);
      setActiveContext(null);
    }
  }, [activeServiceId]);

  useEffect(() => {
    const subscriptions = [
      liveEvents.onTranscriptUpdated((segment) => {
        if (!segment.isFinal) return; // interim text is not added to the permanent feed
        setTranscript((prev) => [...prev.slice(-(TRANSCRIPT_LIMIT - 1)), segment]);
      }),
      liveEvents.onScriptureDetected((detection) => {
        if (detection.context) setActiveContext(detection.context);
      }),
      liveEvents.onScriptureUpdated((detection) => {
        if (detection.context) setActiveContext(detection.context);
      }),
      liveEvents.onSuggestionCreated((s) => setSuggestions((prev) => [s, ...prev])),
      liveEvents.onSuggestionApproved((s) => setSuggestions((prev) => prev.filter((x) => x.id !== s.id))),
      liveEvents.onSuggestionRejected((s) => setSuggestions((prev) => prev.filter((x) => x.id !== s.id))),
      liveEvents.onSuggestionEdited((s) => setSuggestions((prev) => prev.map((x) => (x.id === s.id ? s : x)))),
    ];
    return () => {
      subscriptions.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, []);

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
              onClick={() => withBusy("start-service", async () => {
                await commands.startService(serviceTitle);
                refreshStatus();
              })}
            >
              Start Service
            </button>
          </div>
        ) : (
          <div className="live-brain__row">
            <span>
              {status.service.title} &mdash; <strong>{status.serviceStatus.toUpperCase()}</strong>
            </span>
            <button
              type="button"
              disabled={isBusy("end-service")}
              onClick={() => withBusy("end-service", async () => {
                await commands.endService();
                refreshStatus();
              })}
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
              onClick={() => withBusy("stop-listening", async () => {
                await commands.stopListening();
                refreshStatus();
              })}
            >
              Stop Listening
            </button>
          ) : (
            <button
              type="button"
              disabled={!status?.service || status.speechStatus === "unavailable" || isBusy("start-listening")}
              onClick={() => withBusy("start-listening", async () => {
                await commands.startListening(selectedDevice || undefined);
                refreshStatus();
              })}
            >
              Start Listening
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
              onClick={() => withBusy("manual-transcript", async () => {
                await commands.processTestTranscript(manualText.trim());
                setManualText("");
              })}
            >
              Submit
            </button>
          </div>
        </details>
      </section>

      <section className="live-brain__panel">
        <h2>Live Transcript</h2>
        {transcript.length === 0 ? (
          <p className="live-brain__hint">Nothing transcribed yet.</p>
        ) : (
          <ul className="live-brain__transcript">
            {transcript.map((segment) => (
              <li key={segment.id}>&ldquo;{segment.text}&rdquo;</li>
            ))}
          </ul>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>Active Scripture</h2>
        {activeContext ? (
          <p className="live-brain__active-context">
            {activeContext.book} {activeContext.chapter}
            {activeContext.lastVerse ? `:${activeContext.lastVerse}` : ""}
          </p>
        ) : (
          <p className="live-brain__hint">No active context.</p>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>Scripture Detected</h2>
        {suggestions.length === 0 ? (
          <p className="live-brain__hint">No pending suggestions.</p>
        ) : (
          <ul className="live-brain__suggestions">
            {suggestions.map((s) => (
              <SuggestionCard
                key={s.id}
                suggestion={s}
                busy={busy}
                editingId={editingId}
                editValue={editValue}
                onEditValueChange={setEditValue}
                onStartEdit={(id, currentReference) => {
                  setEditingId(id);
                  setEditValue(currentReference);
                }}
                onCancelEdit={() => setEditingId(null)}
                onApprove={(id) => withBusy(`approve-${id}`, async () => {
                  await commands.approveSuggestion(id);
                })}
                onSaveEdit={(id) => withBusy(`edit-${id}`, async () => {
                  await commands.editSuggestion(id, editValue.trim());
                  setEditingId(null);
                })}
                onReject={(id) => withBusy(`reject-${id}`, async () => {
                  await commands.rejectSuggestion(id);
                })}
                onPrepare={(id) => withBusy(`prepare-${id}`, async () => {
                  await commands.preparePresentation(id);
                })}
              />
            ))}
          </ul>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>Manual Bible Search</h2>
        <p className="live-brain__hint">Works with no speech model, no audio device, and no internet connection.</p>
        <div className="live-brain__row">
          <input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="e.g. Romans 8:28"
            aria-label="Bible search query"
          />
          <button
            type="button"
            disabled={!searchQuery.trim() || isBusy("search")}
            onClick={() => withBusy("search", async () => {
              const results = await commands.searchBible(searchQuery.trim());
              setSearchResults(results);
            })}
          >
            Search
          </button>
        </div>
        {searchResults.length > 0 && (
          <ul className="live-brain__search-results">
            {searchResults.map((verse) => (
              <li key={`${verse.reference.book}-${verse.reference.chapter}-${verse.reference.verseStart}`}>
                <strong>
                  {verse.reference.book} {verse.reference.chapter}:{verse.reference.verseStart}
                </strong>
                <span> &mdash; {verse.text}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="live-brain__panel">
        <h2>Current Output</h2>
        <p className="live-brain__hint">Nothing projected. CIP never projects content automatically.</p>
      </section>
    </div>
  );
}

function StatusBar({ status }: { status: LiveStatus | null }) {
  if (!status) return <p className="live-brain__hint">Connecting to backend&hellip;</p>;
  return (
    <div className="live-brain__status-bar">
      <StatusBadge label="Network" value={status.networkStatus} good={["online"]} />
      <StatusBadge label="Audio" value={status.audioStatus} good={["ready", "listening"]} />
      <StatusBadge label="Speech" value={status.speechStatus} good={["ready"]} />
      <StatusBadge label="AI" value={status.aiStatus} good={["available"]} />
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

interface SuggestionCardProps {
  suggestion: Suggestion;
  busy: string | null;
  editingId: string | null;
  editValue: string;
  onEditValueChange: (value: string) => void;
  onStartEdit: (id: string, currentReference: string) => void;
  onCancelEdit: () => void;
  onApprove: (id: string) => void;
  onSaveEdit: (id: string) => void;
  onReject: (id: string) => void;
  onPrepare: (id: string) => void;
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
  onPrepare,
}: SuggestionCardProps) {
  const reference = suggestion.kind.type === "scripture" ? suggestion.kind.reference : suggestion.kind.label;
  const isEditing = editingId === suggestion.id;
  const confidencePercent = Math.round(suggestion.confidence.score * 100);

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
          <div className="live-brain__row">
            <button type="button" disabled={busy === `prepare-${suggestion.id}`} onClick={() => onPrepare(suggestion.id)}>
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
        </>
      )}
    </li>
  );
}
