/**
 * Service Replay (Phase 3.8). The professional, operator-facing successor
 * to Phase 3.7's Offline Test Center - reorganized, not replaced (spec
 * section 36): every scenario/manual-entry capability that screen already
 * had is preserved here, plus real sequential transcript replay.
 *
 * CORE RULE (spec section 13): Replay is an INPUT ADAPTER, never an
 * intelligence engine. Every action below calls an *existing* production
 * command - `processTestTranscript` (Bible Suggestion path, the exact
 * pipeline live speech already uses), `analyzeBibleTranscript` (Bible
 * Finding path, so replayed Scripture can also participate in Cross-
 * Domain/Content correlation), `analyzeSermonTranscript`,
 * `analyzeMusicTranscript`, `analyzeCrossDomain`, `analyzeContentIntelligence`
 * - all pre-existing, all real. This component never introduces a second
 * intelligence engine, a second Bible provider, or a fabricated finding.
 * Segment scheduling (pause/resume/stop/restart/speed) lives entirely in
 * this component's own React state - nothing about "a replay is in
 * progress" is known to, or persisted by, the backend (spec section 28:
 * replay cursor/state stays in memory only).
 *
 * SEMANTIC RULE (spec section 2): replay is never presented as live
 * microphone/audio input. Every screen here is explicitly labeled
 * "SERVICE REPLAY - Simulated live transcript," distinct from the
 * separate, smaller-grain "Manual Transcript" box (a single line, not a
 * scheduled sequence) also on this screen.
 *
 * Results are reviewed on the Live Service tab (Attention Queue/
 * Intelligence Feed) exactly as they already are for live speech and
 * manual entry - this component deliberately does not duplicate that
 * display.
 */
import { useEffect, useRef, useState } from "react";
import * as commands from "../../lib/commands";
import { delayForSpeed, segmentTranscript, type ReplaySpeed } from "./replay";

interface Scenario {
  id: string;
  label: string;
  domain: string;
  steps: Array<{ description: string; text: string; kind: "bible" | "sermon" | "music" }>;
  runsCrossDomain?: boolean;
  expects: string;
}

const SCENARIOS: Scenario[] = [
  {
    id: "scripture",
    label: "1 · Scripture",
    domain: "Bible",
    steps: [{ description: "Bible pipeline", text: "Please turn to James chapter 2 verse 2.", kind: "bible" }],
    expects: "Expected: a Bible detection/suggestion for James 2:2, reviewable in Live Service's Attention Queue.",
  },
  {
    id: "scripture-context",
    label: "2 · Scripture + Context",
    domain: "Bible",
    steps: [
      {
        description: "Bible pipeline",
        text: "Turn with me to James chapter 2. As we think about what it means to show no partiality, let's read together starting at verse 2.",
        kind: "bible",
      },
    ],
    expects: "Expected: Bible context resolution across the surrounding sermon language, not just a bare reference.",
  },
  {
    id: "sermon",
    label: "3 · Sermon",
    domain: "Sermon",
    steps: [
      {
        description: "Sermon Intelligence pipeline",
        text: "Today I want to talk about faithfulness. The theme of our message is faithfulness in small things. My main point is this: God notices what we do when no one is watching.",
        kind: "sermon",
      },
    ],
    expects: "Expected: Sermon Intelligence theme/point findings, reviewable in Diagnostics → Sermon Intelligence.",
  },
  {
    id: "multi-domain",
    label: "4 · Multi-Domain",
    domain: "Bible + Music + Sermon + Cross-Domain",
    steps: [
      {
        description: "Bible pipeline",
        text: "Let's turn to Romans chapter 8 verse 28, and we know that all things work together for good.",
        kind: "bible",
      },
      { description: "Sermon pipeline", text: "This is our main point today: God works all things for good.", kind: "sermon" },
      { description: "Music pipeline", text: "Let's sing Test Fixture Hymn One together.", kind: "music" },
    ],
    runsCrossDomain: true,
    expects:
      "Expected: multiple domain findings close together in the transcript, then a cross-domain correlation if the deterministic rule engine's proximity/reference rules genuinely match (see core/intelligence/src/cross_domain.rs) - the music line only matches in a development/test environment with the dev fixture dataset seeded, honestly NOT in a real production install with no licensed music library.",
  },
  {
    id: "presentation",
    label: "5 · Presentation",
    domain: "Bible → Presentation",
    steps: [{ description: "Bible pipeline", text: "Turn to John chapter 3 verse 16.", kind: "bible" }],
    expects:
      "Expected: a Bible suggestion for John 3:16 - approve it in the Attention Queue, then Prepare/Display it from the Presentation card, all without any microphone.",
  },
];

const FULL_SERVICE_STEPS: Array<{ description: string; text: string; kind: "bible" | "sermon" }> = [
  { description: "Welcome", text: "Good morning everyone, welcome to church today.", kind: "sermon" },
  { description: "Worship", text: "Let's stand and worship together this morning.", kind: "sermon" },
  { description: "Scripture", text: "Please turn with me to Psalm 23 verse 1.", kind: "bible" },
  {
    description: "Sermon",
    text: "Today's message is about trust. The Lord is our shepherd; we lack nothing when we trust Him.",
    kind: "sermon",
  },
  { description: "Prayer", text: "Let's bow our heads and pray together.", kind: "sermon" },
  { description: "Closing", text: "Thank you for joining us today, go in peace.", kind: "sermon" },
];

/** Spec section 19's sample - deliberately unmodified beyond paragraph
 * breaks for segmentation. Never represented as a real sermon. */
const SAMPLE_TRANSCRIPT = `Good morning church. Today I want us to remember the faithfulness of God.

John chapter 3 verse 16 reminds us of God's love for the world.

When we face difficult seasons, we should remember Romans chapter 8 verse 28.

Scripture teaches us that God works through every circumstance for the good of those who love Him.

Today I want us to move from fear to faith.

Let us pray.`;

function ReadinessRow({ label, ready, optional }: { label: string; ready: boolean; optional?: boolean }) {
  const tone = ready ? "good" : optional ? "warn" : "bad";
  return (
    <span className={`op-status-strip__item op-status-strip__item--${tone}`}>
      <span className="op-status-strip__dot" aria-hidden="true" />
      {label} {ready ? "Ready" : optional ? "Optional — not configured" : "Not ready"}
    </span>
  );
}

interface ReplayRunState {
  playing: boolean;
  paused: boolean;
  cancelled: boolean;
  index: number;
}

export function ServiceReplay() {
  const [bibleReady, setBibleReady] = useState(false);
  const [micReady, setMicReady] = useState(false);
  const [speechReady, setSpeechReady] = useState(false);
  const [serviceActive, setServiceActive] = useState(false);
  const [serviceTitle, setServiceTitle] = useState("Service Replay");
  const [manualText, setManualText] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [transcriptText, setTranscriptText] = useState("");
  const [segments, setSegments] = useState<string[]>([]);
  const [currentIndex, setCurrentIndex] = useState(0);
  const [replayPlaying, setReplayPlaying] = useState(false);
  const [replayPaused, setReplayPaused] = useState(false);
  const [speed, setSpeed] = useState<ReplaySpeed>(1);
  const runRef = useRef<ReplayRunState>({ playing: false, paused: false, cancelled: false, index: 0 });
  const fileInputRef = useRef<HTMLInputElement>(null);

  const refreshReadiness = () => {
    commands
      .getLiveStatus()
      .then((status) => {
        setBibleReady(!!status.bible && status.bible.status === "enabled");
        setSpeechReady(status.speechStatus === "ready");
        setServiceActive(!!status.service && status.serviceStatus !== "completed");
      })
      .catch(() => {});
    commands
      .listAudioDevices()
      .then((devices) => setMicReady(devices.length > 0))
      .catch(() => {});
  };

  useEffect(() => {
    refreshReadiness();
  }, []);

  const appendLog = (line: string) => setLog((prev) => [`${new Date().toLocaleTimeString()} — ${line}`, ...prev].slice(0, 60));

  const withBusy = async (key: string, action: () => Promise<void>) => {
    setBusy(key);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(String(e));
      appendLog(`FAILED: ${String(e)}`);
    } finally {
      setBusy(null);
    }
  };

  const startTestService = () =>
    withBusy("start-service", async () => {
      await commands.startService(serviceTitle);
      appendLog(`Started service "${serviceTitle}".`);
      refreshReadiness();
    });

  const endTestService = () =>
    withBusy("end-service", async () => {
      stopReplay();
      await commands.endService();
      appendLog("Ended service. Review it under History.");
      refreshReadiness();
    });

  const runStep = async (step: { description: string; text: string; kind: "bible" | "sermon" | "music" }) => {
    if (step.kind === "bible") {
      await commands.processTestTranscript(step.text);
    } else if (step.kind === "sermon") {
      await commands.analyzeSermonTranscript(step.text);
    } else {
      await commands.analyzeMusicTranscript(step.text);
    }
    appendLog(`Submitted (${step.description}): "${step.text}"`);
  };

  const runScenario = (scenario: Scenario) =>
    withBusy(`scenario-${scenario.id}`, async () => {
      if (!serviceActive) {
        throw new Error("Start a service first - every scenario needs an active service, exactly like real speech would.");
      }
      for (const step of scenario.steps) {
        await runStep(step);
      }
      if (scenario.runsCrossDomain) {
        const correlations = await commands.analyzeCrossDomain();
        appendLog(`Ran cross-domain analysis — ${correlations.length} correlation(s) found.`);
      }
      appendLog(`Scenario "${scenario.label}" submitted. Review Live Service → Attention Queue / Intelligence Feed.`);
    });

  const runFullService = () =>
    withBusy("full-service", async () => {
      await commands.startService("Offline Test Service — Full Run");
      appendLog("Started full-service scenario.");
      for (const step of FULL_SERVICE_STEPS) {
        if (step.kind === "bible") {
          await commands.processTestTranscript(step.text);
        } else {
          await commands.analyzeSermonTranscript(step.text);
        }
        appendLog(`${step.description}: "${step.text}"`);
      }
      await commands.endService();
      appendLog("Full-service scenario complete and stopped. Review it under History.");
      refreshReadiness();
    });

  const submitManual = () =>
    withBusy("manual", async () => {
      if (!manualText.trim()) return;
      await commands.processTestTranscript(manualText.trim());
      appendLog(`Manual transcript submitted: "${manualText.trim()}"`);
      setManualText("");
    });

  // --- Service Replay: sequential, timed, pausable segment scheduler ----

  const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

  const processReplaySegment = async (text: string) => {
    // Three independent, pre-existing production entry points, called in
    // order for the same real text: the Bible Suggestion path (what the
    // operator reviews/approves/prepares/presents), the Bible Finding
    // path (so a replayed Scripture reference can also participate in
    // Cross-Domain/Content correlation, exactly like `analyzeBibleTranscript`
    // already exists to allow), and the Sermon path. Never a fabricated
    // or duplicated intelligence pathway - each call is the same command
    // an operator could type into a box themselves.
    await commands.processTestTranscript(text);
    await commands.analyzeBibleTranscript(text);
    await commands.analyzeSermonTranscript(text);
    appendLog(`Replayed segment: "${text}"`);
  };

  const playLoop = async () => {
    const run = runRef.current;
    while (run.index < segments.length && !run.cancelled) {
      if (run.paused) {
        await sleep(200);
        continue;
      }
      const text = segments[run.index];
      try {
        await processReplaySegment(text);
      } catch (e) {
        appendLog(`Replay segment failed (continuing): ${String(e)}`);
      }
      if (run.cancelled) break;
      run.index += 1;
      setCurrentIndex(run.index);
      if (run.index < segments.length && !run.cancelled) {
        await sleep(delayForSpeed(speed));
      }
    }
    if (!run.cancelled) {
      appendLog("Service Replay complete. Review Live Service → Attention Queue / Intelligence Feed.");
    }
    run.playing = false;
    setReplayPlaying(false);
    setReplayPaused(false);
  };

  const startReplay = () =>
    withBusy("replay-start", async () => {
      if (!serviceActive) {
        throw new Error("Start a service first - Service Replay needs an active service, exactly like real speech would.");
      }
      const parsed = segmentTranscript(transcriptText);
      if (parsed.length === 0) {
        throw new Error("Enter or load a transcript before starting replay.");
      }
      setSegments(parsed);
      setCurrentIndex(0);
      runRef.current = { playing: true, paused: false, cancelled: false, index: 0 };
      setReplayPlaying(true);
      setReplayPaused(false);
      appendLog(`Started Service Replay — ${parsed.length} segment(s) at ${speed === "instant" ? "instant" : `${speed}x`} speed.`);
      void playLoop();
    });

  const pauseReplay = () => {
    runRef.current.paused = true;
    setReplayPaused(true);
    appendLog("Service Replay paused.");
  };

  const resumeReplay = () => {
    runRef.current.paused = false;
    setReplayPaused(false);
    appendLog("Service Replay resumed.");
  };

  const stopReplay = () => {
    if (!runRef.current.playing) return;
    runRef.current.cancelled = true;
    runRef.current.playing = false;
    setReplayPlaying(false);
    setReplayPaused(false);
    appendLog("Service Replay stopped.");
  };

  const restartReplay = () =>
    withBusy("replay-restart", async () => {
      if (segments.length === 0) return;
      runRef.current.cancelled = true; // stop any in-flight loop first
      await sleep(0);
      runRef.current = { playing: true, paused: false, cancelled: false, index: 0 };
      setCurrentIndex(0);
      setReplayPlaying(true);
      setReplayPaused(false);
      appendLog("Service Replay restarted from segment 1.");
      void playLoop();
    });

  const runCrossDomainAndContent = () =>
    withBusy("replay-correlate", async () => {
      const correlations = await commands.analyzeCrossDomain();
      const candidates = await commands.analyzeContentIntelligence();
      appendLog(`Ran cross-domain + content analysis — ${correlations.length} correlation(s), ${candidates.length} candidate(s).`);
    });

  const loadSampleTranscript = () => {
    setTranscriptText(SAMPLE_TRANSCRIPT);
    appendLog("Loaded sample/demonstration transcript.");
  };

  const onFileSelected = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      setTranscriptText(String(reader.result ?? ""));
      appendLog(`Loaded transcript file "${file.name}".`);
    };
    reader.onerror = () => setError(`Could not read file "${file.name}".`);
    reader.readAsText(file);
    if (fileInputRef.current) fileInputRef.current.value = "";
  };

  const isBusy = (key: string) => busy === key;
  const progressPct = segments.length > 0 ? Math.round((currentIndex / segments.length) * 100) : 0;

  return (
    <div className="library-page library-page--history">
      <header className="library-page__header">
        <div>
          <p className="library-page__eyebrow">Service Replay</p>
          <h1>Test Without Hardware</h1>
        </div>
      </header>
      <p className="library-page__status-line">
        Every action below uses CIP's real production intelligence pipeline - no fake data, no separate test engine.
        Works fully offline: no microphone, no Whisper model, no Internet, and no projector required.
      </p>

      {error && (
        <p className="live-brain__error" role="alert">
          {error}
        </p>
      )}

      <section className="library-panel">
        <h2>Core Offline Readiness</h2>
        <div className="op-status-strip">
          <ReadinessRow label="Bible" ready={bibleReady} />
          <ReadinessRow label="Manual Input" ready />
          <ReadinessRow label="Presentation" ready />
          <ReadinessRow label="Microphone" ready={micReady} optional />
          <ReadinessRow label="Speech (Whisper)" ready={speechReady} optional />
        </div>
        <p className="live-brain__hint" style={{ marginTop: "0.5rem" }}>
          Microphone and Speech are optional live-input hardware - everything below works without them.
        </p>
      </section>

      <section className="library-panel">
        <h2>Test Service</h2>
        {!serviceActive ? (
          <div className="live-brain__row">
            <input
              value={serviceTitle}
              onChange={(e) => setServiceTitle(e.target.value)}
              aria-label="Test service title"
            />
            <button type="button" className="op-button--primary" disabled={isBusy("start-service")} onClick={startTestService}>
              Start Service
            </button>
          </div>
        ) : (
          <div className="live-brain__row">
            <span className="op-badge op-badge--live">● Live — Service Active</span>
            <button type="button" className="op-button--danger" disabled={isBusy("end-service")} onClick={endTestService}>
              End Service
            </button>
          </div>
        )}
      </section>

      <section className="library-panel">
        <div className="live-brain__row" style={{ justifyContent: "space-between", alignItems: "baseline" }}>
          <h2 style={{ margin: 0 }}>Service Replay</h2>
          <span className="op-badge op-badge--live">SERVICE REPLAY — Simulated live transcript</span>
        </div>
        <p className="live-brain__hint">
          Replay mode simulates a live service from a transcript, feeding it through CIP's real intelligence pipeline
          one segment at a time, in order, exactly as a pastor speaking would arrive. <strong>It does not provide real
          microphone/audio evidence</strong> - nothing here is Whisper transcription, and no finding produced this way
          should ever be described as "detected from live audio."
        </p>

        <div className="live-brain__row">
          <button type="button" onClick={loadSampleTranscript} disabled={replayPlaying}>
            Load Sample Transcript
          </button>
          <button type="button" onClick={() => fileInputRef.current?.click()} disabled={replayPlaying}>
            Load .txt / .md File
          </button>
          <input
            ref={fileInputRef}
            type="file"
            accept=".txt,.md,text/plain,text/markdown"
            onChange={onFileSelected}
            style={{ display: "none" }}
            aria-label="Load transcript file"
          />
        </div>

        <textarea
          value={transcriptText}
          onChange={(e) => setTranscriptText(e.target.value)}
          disabled={replayPlaying}
          placeholder="Paste a service or sermon transcript here, or load the sample above."
          rows={8}
          aria-label="Service Replay transcript"
          style={{ width: "100%", marginTop: "0.5rem" }}
        />

        <div className="live-brain__row" style={{ marginTop: "0.5rem" }}>
          <label htmlFor="replay-speed" className="live-brain__label">
            Speed
          </label>
          <select
            id="replay-speed"
            value={String(speed)}
            onChange={(e) => setSpeed(e.target.value === "instant" ? "instant" : (Number(e.target.value) as ReplaySpeed))}
            disabled={replayPlaying}
          >
            <option value="0.25">0.25x</option>
            <option value="0.5">0.5x</option>
            <option value="1">1x</option>
            <option value="2">2x</option>
            <option value="4">4x</option>
            <option value="instant">Instant</option>
          </select>

          {!replayPlaying ? (
            <button type="button" className="op-button--primary" disabled={isBusy("replay-start") || !transcriptText.trim()} onClick={startReplay}>
              Start Replay
            </button>
          ) : replayPaused ? (
            <button type="button" className="op-button--primary" onClick={resumeReplay}>
              Resume
            </button>
          ) : (
            <button type="button" onClick={pauseReplay}>
              Pause
            </button>
          )}
          <button type="button" className="op-button--danger" disabled={!replayPlaying} onClick={stopReplay}>
            Stop
          </button>
          <button type="button" disabled={segments.length === 0 || isBusy("replay-restart")} onClick={restartReplay}>
            Restart
          </button>
          <button type="button" disabled={isBusy("replay-correlate")} onClick={runCrossDomainAndContent}>
            Analyze Cross-Domain + Content
          </button>
        </div>

        {segments.length > 0 && (
          <div style={{ marginTop: "0.5rem" }}>
            <p className="live-brain__hint">
              Segment {Math.min(currentIndex + (replayPlaying ? 1 : 0), segments.length)} of {segments.length}
              {replayPaused ? " — paused" : replayPlaying ? " — playing" : " — stopped"}
            </p>
            <div style={{ background: "var(--surface-2, #22283a)", borderRadius: "4px", height: "6px", overflow: "hidden" }}>
              <div
                style={{
                  width: `${progressPct}%`,
                  background: "var(--status-live, #7db6ff)",
                  height: "100%",
                  transition: "width 0.2s ease",
                }}
              />
            </div>
          </div>
        )}
      </section>

      <section className="library-panel">
        <h2>Manual Transcript</h2>
        <p className="live-brain__hint">
          A single line, submitted once - distinct from Service Replay above. Enters the exact same Bible Intelligence
          pipeline real speech uses.
        </p>
        <div className="live-brain__row">
          <input
            value={manualText}
            onChange={(e) => setManualText(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && submitManual()}
            placeholder='e.g. "Turn with me to Romans chapter 8."'
            aria-label="Manual transcript text"
          />
          <button type="button" className="op-button--primary" disabled={!manualText.trim() || isBusy("manual")} onClick={submitManual}>
            Submit
          </button>
        </div>
      </section>

      <section className="library-panel">
        <h2>Quick Scenarios</h2>
        <p className="live-brain__hint">
          Deterministic, pre-written transcripts exercising real domain engines. A scenario submits text; whether a
          domain engine actually finds something depends on that engine's real detection logic, never guaranteed
          here.
        </p>
        <ul className="library-card-list">
          {SCENARIOS.map((s) => (
            <li key={s.id} className="library-card library-card--bible">
              <div className="library-card__header">
                <strong>{s.label}</strong>
                <span className="library-card__meta">{s.domain}</span>
              </div>
              <p className="library-card__text">{s.expects}</p>
              <div className="library-card__actions">
                <button type="button" disabled={isBusy(`scenario-${s.id}`)} onClick={() => runScenario(s)}>
                  Run Scenario
                </button>
              </div>
            </li>
          ))}
          <li className="library-card library-card--bible">
            <div className="library-card__header">
              <strong>6 · Full Service</strong>
              <span className="library-card__meta">Service + Bible + Sermon</span>
            </div>
            <p className="library-card__text">
              Runs a complete deterministic sequence: Start Service → Welcome → Worship → Scripture →
              Sermon → Prayer → Closing → Stop Service. Ends with a real, reviewable entry in History.
            </p>
            <div className="library-card__actions">
              <button type="button" className="op-button--primary" disabled={isBusy("full-service")} onClick={runFullService}>
                Run Full Service
              </button>
            </div>
          </li>
        </ul>
      </section>

      <section className="library-panel">
        <h2>Activity Log</h2>
        {log.length === 0 ? (
          <p className="live-brain__hint">Nothing submitted yet.</p>
        ) : (
          <ul className="live-brain__timeline">
            {log.map((line, i) => (
              <li key={i}>{line}</li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
