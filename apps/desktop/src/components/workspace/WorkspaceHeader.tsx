/**
 * Sermon/service-progress snapshot (Phase 2.9 spec section 5A, trimmed in
 * Phase 3.5.1 - see docs/phase-3-5-1-ux-audit.md finding P0-3). This used
 * to duplicate Service/Bible/Scripture/Audio-Speech/Output/Acoustic, all of
 * which now have a better, more visible home elsewhere on the same screen
 * (`ServiceControlBar`, `SystemStatusStrip`, the "Current Scripture" panel,
 * `PresentationCard`, and the Diagnostics-only Music Intelligence panel for
 * Acoustic). What's left here is the handful of facts nothing else shows:
 * service phase, the active sermon and its speaker, and the confirmed
 * current song. Shows "Unknown"/"None" rather than guessing when a fact is
 * not yet available (spec rule 5: "Unknown speaker remains Unknown").
 */
import type { LiveStatus, SermonFoundationSummary, ServiceIntelligenceSummary } from "../../domain";

export interface WorkspaceHeaderProps {
  status: LiveStatus | null;
  sermonFoundation: SermonFoundationSummary | null;
  serviceIntel: ServiceIntelligenceSummary | null;
}

export function WorkspaceHeader({ status, sermonFoundation, serviceIntel }: WorkspaceHeaderProps) {
  const sermon = sermonFoundation?.activeSermon;
  return (
    <section className="live-brain__panel workspace-header">
      <h2>Service Snapshot</h2>
      <div className="op-status-strip">
        <span className="op-status-strip__item op-status-strip__item--neutral">
          Phase&nbsp;<strong>{serviceIntel?.phase ?? "Unknown"}</strong>
        </span>
        <span className="op-status-strip__item op-status-strip__item--neutral">
          Sermon&nbsp;<strong>{sermon ? (sermon.title ?? "Untitled") : "None active"}</strong>
        </span>
        <span className="op-status-strip__item op-status-strip__item--neutral">
          Speaker&nbsp;<strong>{sermon?.speaker?.name ?? "Unknown"}</strong>
        </span>
        <span className="op-status-strip__item op-status-strip__item--neutral">
          Song&nbsp;<strong>{status?.currentSong ? status.currentSong.songId : "None confirmed"}</strong>
        </span>
      </div>
    </section>
  );
}
