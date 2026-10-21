import type { ContentMetadata } from "../../domain";

export interface ReadinessCenterProps {
  bible: ContentMetadata | null;
  deviceCount: number;
  speechReady: boolean;
  displayOpen: boolean;
  onOpenDiagnostics: () => void;
}

function ReadinessRow({ label, ready, detail }: { label: string; ready: boolean; detail: string }) {
  return (
    <li className="readiness-center__row">
      <span className={`readiness-center__indicator ${ready ? "readiness-center__indicator--ready" : ""}`} aria-hidden="true" />
      <span className="readiness-center__copy">
        <strong>{label}</strong>
        <small>{detail}</small>
      </span>
      <span className="readiness-center__state">{ready ? "Ready" : "Needs attention"}</span>
    </li>
  );
}

export function ReadinessCenter({ bible, deviceCount, speechReady, displayOpen, onOpenDiagnostics }: ReadinessCenterProps) {
  const readyCount = [Boolean(bible && bible.status === "enabled"), deviceCount > 0, speechReady, displayOpen].filter(Boolean).length;

  return (
    <details className="readiness-center">
      <summary>
        <span>
          <strong>Readiness Center</strong>
          <small>{readyCount}/4 live-service systems ready</small>
        </span>
        <span className="readiness-center__summary-state">{readyCount === 4 ? "Ready to run" : "Review setup"}</span>
      </summary>
      <ul className="readiness-center__list">
        <ReadinessRow
          label="Bible data"
          ready={Boolean(bible && bible.status === "enabled")}
          detail={bible ? `${bible.name} · ${bible.licensingStatus.replaceAll("_", " ")}` : "No enabled Bible dataset"}
        />
        <ReadinessRow label="Audio input" ready={deviceCount > 0} detail={deviceCount > 0 ? `${deviceCount} input device(s) detected` : "Connect a microphone or mixer feed"} />
        <ReadinessRow label="Speech recognition" ready={speechReady} detail={speechReady ? "Whisper engine is ready" : "Install or select a speech model in Diagnostics"} />
        <ReadinessRow label="Presentation output" ready={displayOpen} detail={displayOpen ? "A presentation display is open" : "Open a display when you are ready to project"} />
      </ul>
      <button type="button" onClick={onOpenDiagnostics}>Open detailed diagnostics</button>
    </details>
  );
}
