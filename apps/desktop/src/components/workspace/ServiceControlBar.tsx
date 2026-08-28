/**
 * Phase 3.5: the service control bar (spec section 6/10, Priority 1 -
 * "always obvious: READY / SERVICE LIVE / PAUSED / NO ACTIVE SERVICE").
 * Two states, both built entirely from state `LiveChurchBrain` already
 * fetches (`status`, `devices`, `appConfig`) and calling exactly the
 * commands the old inline "Service" panel already called
 * (`startService`/`pauseService`/`resumeService`/`endService`) - no new
 * command, no new event.
 *
 * Before a service starts, this is the first-launch "READY TO START"
 * experience (spec section 6): a real call to action instead of a small
 * grey "no active service" line, plus a compact readiness summary so an
 * operator can see at a glance whether anything needs attention before
 * they begin - never a scary "broken" look for something merely optional
 * (e.g. no Whisper model configured).
 */
import type { AudioDevice, ContentMetadata } from "../../domain";
import type { AppConfig } from "../../config/appConfig";

export interface ServiceControlBarProps {
  isActive: boolean;
  serviceTitle: string;
  activeTitle: string | null;
  serviceStatus: "planned" | "live" | "paused" | "completed" | null;
  bible: ContentMetadata | null;
  devices: AudioDevice[];
  speechReady: boolean;
  appConfig: AppConfig | null;
  busy: string | null;
  onTitleChange: (title: string) => void;
  onStart: () => void;
  onPause: () => void;
  onResume: () => void;
  onEnd: () => void;
}

function ReadinessItem({ label, ready, optional }: { label: string; ready: boolean; optional?: boolean }) {
  const tone = ready ? "good" : optional ? "warn" : "bad";
  return (
    <span className={`op-status-strip__item op-status-strip__item--${tone}`}>
      <span className="op-status-strip__dot" aria-hidden="true" />
      {label} {ready ? "Ready" : optional ? "Optional" : "Not ready"}
    </span>
  );
}

export function ServiceControlBar({
  isActive,
  serviceTitle,
  activeTitle,
  serviceStatus,
  bible,
  devices,
  speechReady,
  appConfig,
  busy,
  onTitleChange,
  onStart,
  onPause,
  onResume,
  onEnd,
}: ServiceControlBarProps) {
  const isBusy = (key: string) => busy === key;

  if (!isActive) {
    return (
      <section className="op-hero">
        <p className="op-hero__eyebrow">Church Intelligence Platform</p>
        <h1 className="op-hero__title">Ready to Start</h1>
        <p className="op-hero__body">
          Start a new church service and CIP will assist with Scripture, music, sermon, and presentation
          intelligence as the service happens &mdash; you stay in control of everything shown on screen.
        </p>
        <div className="op-hero__form">
          <input
            value={serviceTitle}
            onChange={(e) => onTitleChange(e.target.value)}
            placeholder="Service title (e.g. Sunday Morning Service)"
            aria-label="Service title"
          />
          <button type="button" className="op-button--primary" disabled={isBusy("start-service")} onClick={onStart}>
            Start Service
          </button>
        </div>
        <div className="op-readiness">
          <ReadinessItem label="Bible" ready={!!bible && bible.status === "enabled"} />
          <ReadinessItem label="Microphone" ready={devices.length > 0} optional />
          <ReadinessItem label="Speech" ready={speechReady} optional />
        </div>
        {devices.length === 0 && (
          <p className="live-brain__hint" style={{ marginTop: "0.75rem" }}>
            No microphone detected yet &mdash; you can still run a service with manual transcript entry.
          </p>
        )}
        {!speechReady && appConfig?.whisperModelPath && (
          <p className="live-brain__hint" style={{ marginTop: "0.35rem" }}>
            Speech recognition is optional. To enable it, place a Whisper model at{" "}
            <code>{appConfig.whisperModelPath}</code> and restart CIP.
          </p>
        )}
      </section>
    );
  }

  return (
    <section className="op-service-bar">
      <div className="op-service-bar__identity">
        <span className="op-service-bar__title">{activeTitle}</span>
        <span className={`op-badge ${serviceStatus === "live" ? "op-badge--good" : "op-badge--warn"}`}>
          {serviceStatus === "live" ? "● Live" : "Paused"}
        </span>
      </div>
      <div className="live-brain__row" style={{ marginTop: 0 }}>
        {serviceStatus === "live" && (
          <button type="button" disabled={isBusy("pause-service")} onClick={onPause}>
            Pause
          </button>
        )}
        {serviceStatus === "paused" && (
          <button type="button" disabled={isBusy("resume-service")} onClick={onResume}>
            Resume
          </button>
        )}
        <button type="button" className="op-button--danger" disabled={isBusy("end-service")} onClick={onEnd}>
          End Service
        </button>
      </div>
    </section>
  );
}
