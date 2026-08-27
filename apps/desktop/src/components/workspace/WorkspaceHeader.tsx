/**
 * The always-visible, at-a-glance service state (Phase 2.9 spec section
 * 5A). Every field here is read directly from state the existing panels
 * already fetch/subscribe to (`LiveStatus`, `SermonFoundationSummary`,
 * `ServiceIntelligenceSummary`, the active Scripture context, the current
 * presentation output) - this component performs no inference of its
 * own, and shows "Unknown"/"None" rather than guessing when a fact is not
 * yet available (spec rule 5: "Unknown speaker remains Unknown").
 */
import type {
  LiveStatus,
  PresentationItem,
  ScriptureContext,
  ScriptureReference,
  SermonFoundationSummary,
  ServiceIntelligenceSummary,
} from "../../domain";

function referenceDisplay(ref: ScriptureReference): string {
  return `${ref.book} ${ref.chapter}:${ref.verseStart}`;
}

export interface WorkspaceHeaderProps {
  status: LiveStatus | null;
  sermonFoundation: SermonFoundationSummary | null;
  serviceIntel: ServiceIntelligenceSummary | null;
  activeContext: ScriptureContext | null;
  lastReference: ScriptureReference | null;
  activeDisplayItem: PresentationItem | null;
  displayWindowOpen: boolean;
}

export function WorkspaceHeader({
  status,
  sermonFoundation,
  serviceIntel,
  activeContext,
  lastReference,
  activeDisplayItem,
  displayWindowOpen,
}: WorkspaceHeaderProps) {
  const sermon = sermonFoundation?.activeSermon;
  return (
    <section className="live-brain__panel workspace-header">
      <h2>Live Service</h2>
      <dl className="workspace-header__grid">
        <div>
          <dt>Service</dt>
          <dd>
            {status?.service ? `${status.service.title} — ${status.serviceStatus.toUpperCase()}` : "No active service"}
          </dd>
        </div>
        <div>
          <dt>Phase</dt>
          <dd>{(serviceIntel?.phase ?? "unknown").toUpperCase()}</dd>
        </div>
        <div>
          <dt>Sermon</dt>
          <dd>{sermon ? `${sermon.title ?? "(untitled)"} — ${sermon.status.toUpperCase()}` : "No active sermon"}</dd>
        </div>
        <div>
          <dt>Speaker</dt>
          <dd>{sermon?.speaker?.name ?? "Unknown"}</dd>
        </div>
        <div>
          <dt>Current song</dt>
          <dd>{status?.currentSong ? status.currentSong.songId : "None confirmed"}</dd>
        </div>
        <div>
          <dt>Bible</dt>
          <dd>
            {status?.bible
              ? `${status.bible.name} — ${status.bible.status.toUpperCase()} (${status.bible.licensingStatus.replace(/_/g, " ").toUpperCase()})`
              : "NOT AVAILABLE"}
          </dd>
        </div>
        <div>
          <dt>Scripture</dt>
          <dd>
            {lastReference
              ? referenceDisplay(lastReference)
              : activeContext
                ? `${activeContext.book} ${activeContext.chapter}`
                : "None yet"}
          </dd>
        </div>
        <div>
          <dt>Audio / Speech</dt>
          <dd title={status?.audio.streamError ?? undefined}>
            {(status?.audioStatus ?? "unknown").toUpperCase()} / {(status?.speechStatus ?? "unknown").toUpperCase()}
            {status?.audio.streamError ? ` — ${status.audio.streamError}` : ""}
          </dd>
        </div>
        <div>
          <dt>Acoustic</dt>
          <dd>{(status?.acousticStatus.status ?? "unknown").toUpperCase()}</dd>
        </div>
        <div>
          <dt>Output</dt>
          <dd>
            {activeDisplayItem
              ? "ACTIVE — ON SCREEN"
              : displayWindowOpen
                ? "OPEN, NOTHING DISPLAYED"
                : "CLOSED"}
          </dd>
        </div>
      </dl>
    </section>
  );
}
