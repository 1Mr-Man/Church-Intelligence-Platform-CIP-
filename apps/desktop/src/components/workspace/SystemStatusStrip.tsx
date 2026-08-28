/**
 * Phase 3.5: the compact, human-language system status row (spec section
 * 5 Priority 6 / section 10's "SYSTEM" example). Deliberately the
 * opposite of the old `StatusBar` it sits above in the render tree
 * (`Runtime: tauri`, `Network: online`, `AI: available`) - every label
 * here is something a church operator already has a mental model for
 * (microphone, speech, Bible, display), and every value is a status word
 * from the shared vocabulary in `docs/phase-3-5-operator-ux.md`, never a
 * raw backend enum. Pure presentational: every fact already lives in
 * `LiveStatus`/`AudioDevice[]`/`displayWindowOpen`, already fetched by
 * `LiveChurchBrain` - this component adds no command, no event, no state
 * of its own.
 */
import type { AudioStatusKind, ContentMetadata, LiveStatus, SpeechStatusKind } from "../../domain";

export interface SystemStatusStripProps {
  status: LiveStatus | null;
  deviceCount: number;
  displayWindowOpen: boolean;
}

type Tone = "good" | "warn" | "bad" | "neutral";

function micTone(audioStatus: AudioStatusKind | undefined, deviceCount: number): { tone: Tone; label: string } {
  if (!audioStatus || deviceCount === 0) return { tone: "warn", label: "Not configured" };
  if (audioStatus === "listening") return { tone: "good", label: "Listening" };
  if (audioStatus === "error") return { tone: "bad", label: "Error" };
  if (audioStatus === "ready") return { tone: "good", label: "Ready" };
  return { tone: "warn", label: "Not configured" };
}

function speechTone(speechStatus: SpeechStatusKind | undefined): { tone: Tone; label: string } {
  if (speechStatus === "ready") return { tone: "good", label: "Ready" };
  if (speechStatus === "error") return { tone: "bad", label: "Error" };
  return { tone: "warn", label: "Optional — not configured" };
}

function bibleTone(bible: ContentMetadata | null | undefined): { tone: Tone; label: string } {
  if (!bible) return { tone: "warn", label: "Not installed" };
  if (bible.status !== "enabled") return { tone: "warn", label: bible.status };
  return { tone: "good", label: `${bible.name} Ready` };
}

function displayTone(open: boolean): { tone: Tone; label: string } {
  return open ? { tone: "good", label: "Open" } : { tone: "neutral", label: "Not open" };
}

export function SystemStatusStrip({ status, deviceCount, displayWindowOpen }: SystemStatusStripProps) {
  const mic = micTone(status?.audioStatus, deviceCount);
  const speech = speechTone(status?.speechStatus);
  const bible = bibleTone(status?.bible);
  const display = displayTone(displayWindowOpen);
  return (
    <div className="op-status-strip" role="status" aria-label="System status">
      <span className={`op-status-strip__item op-status-strip__item--${mic.tone}`}>
        <span className="op-status-strip__icon" aria-hidden="true">
          🎙
        </span>
        <span className="op-status-strip__dot" aria-hidden="true" />
        Microphone {mic.label}
      </span>
      <span className={`op-status-strip__item op-status-strip__item--${speech.tone}`}>
        <span className="op-status-strip__icon" aria-hidden="true">
          🧠
        </span>
        <span className="op-status-strip__dot" aria-hidden="true" />
        Speech {speech.label}
      </span>
      <span className={`op-status-strip__item op-status-strip__item--${bible.tone}`}>
        <span className="op-status-strip__icon" aria-hidden="true">
          📖
        </span>
        <span className="op-status-strip__dot" aria-hidden="true" />
        Bible {bible.label}
      </span>
      <span className={`op-status-strip__item op-status-strip__item--${display.tone}`}>
        <span className="op-status-strip__icon" aria-hidden="true">
          🖥
        </span>
        <span className="op-status-strip__dot" aria-hidden="true" />
        Display {display.label}
      </span>
    </div>
  );
}
