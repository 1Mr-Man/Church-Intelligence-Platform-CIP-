/**
 * Offline Test Center (Phase 3.7). The one, clearly-labeled, top-level
 * destination for proving CIP works without a microphone, without
 * Whisper, without a projector, and without Internet - see
 * docs/phase-3-7-offline-operator-test.md.
 *
 * CORE RULE (spec section 2): every action here calls an *existing*
 * production command - `processTestTranscript` (the same Bible pipeline
 * real speech already uses), `analyzeSermonTranscript`,
 * `analyzeMusicTranscript`, `analyzeCrossDomain`,
 * `analyzeContentIntelligence` (all pre-existing, each already the real
 * manual-entry path for its domain, previously only reachable from inside
 * Diagnostics Mode's per-domain panels). This component never introduces
 * a second intelligence engine, a second Bible provider, or a fabricated
 * finding - it only gives those exact same commands a first-class,
 * discoverable, honestly-labeled home, and reports what it *submitted*,
 * never a guessed outcome (a submission does not guarantee a domain
 * engine finds something - see each scenario's `expects` text).
 *
 * Results are reviewed on the Live Service tab (Attention Queue/
 * Intelligence Feed) exactly as they already are for live speech - this
 * component deliberately does not duplicate that display.
 */
import { useEffect, useState } from "react";
import * as commands from "../../lib/commands";

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
      "Expected: multiple domain findings close together in the transcript, then a cross-domain correlation if the deterministic rule engine's proximity/reference rules genuinely match (see core/intelligence/src/cross_domain.rs) - the music line only matches in a development/test environment with the dev fixture dataset seeded, honestly NOT in a real production install with no licensed music library (see docs/phase-3-7-offline-operator-test.md section 10).",
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

function ReadinessRow({ label, ready, optional }: { label: string; ready: boolean; optional?: boolean }) {
  const tone = ready ? "good" : optional ? "warn" : "bad";
  return (
    <span className={`op-status-strip__item op-status-strip__item--${tone}`}>
      <span className="op-status-strip__dot" aria-hidden="true" />
      {label} {ready ? "Ready" : optional ? "Optional — not configured" : "Not ready"}
    </span>
  );
}

export function TestCenter() {
  const [bibleReady, setBibleReady] = useState(false);
  const [micReady, setMicReady] = useState(false);
  const [speechReady, setSpeechReady] = useState(false);
  const [serviceActive, setServiceActive] = useState(false);
  const [serviceTitle, setServiceTitle] = useState("Offline Test Service");
  const [manualText, setManualText] = useState("");
  const [log, setLog] = useState<string[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  const appendLog = (line: string) => setLog((prev) => [`${new Date().toLocaleTimeString()} — ${line}`, ...prev].slice(0, 30));

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
      appendLog(`Started test service "${serviceTitle}".`);
      refreshReadiness();
    });

  const endTestService = () =>
    withBusy("end-service", async () => {
      await commands.endService();
      appendLog("Ended test service. Review it under History.");
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
        throw new Error("Start a test service first - every scenario needs an active service, exactly like real speech would.");
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

  const isBusy = (key: string) => busy === key;

  return (
    <div className="library-page library-page--history">
      <header className="library-page__header">
        <div>
          <p className="library-page__eyebrow">Offline Test Center</p>
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
              Start Test Service
            </button>
          </div>
        ) : (
          <div className="live-brain__row">
            <span className="op-badge op-badge--live">● Live — Test Service Active</span>
            <button type="button" className="op-button--danger" disabled={isBusy("end-service")} onClick={endTestService}>
              End Test Service
            </button>
          </div>
        )}
      </section>

      <section className="library-panel">
        <h2>Manual Transcript</h2>
        <p className="live-brain__hint">
          Type what a speaker would say - it enters the exact same Bible Intelligence pipeline real speech uses.
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
        <h2>Test Scenarios</h2>
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
