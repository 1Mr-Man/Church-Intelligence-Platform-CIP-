/**
 * The chronological, bounded, cross-domain feed (Phase 2.9 spec section
 * 5C) - Bible/Music/Sermon/Service/Content/Correlation activity in one
 * list, filterable by domain. Read-only: operator actions live in the
 * Attention Queue and the existing per-domain panels below, not here -
 * this region's job is visibility, not a second action surface.
 */
import { useMemo, useState } from "react";
import { IntelligenceCard } from "./IntelligenceCard";
import type { UnifiedIntelligenceDomain, UnifiedIntelligenceItem } from "../../lib/unifiedFeed";

const FILTERS: Array<{ value: UnifiedIntelligenceDomain | "all"; label: string }> = [
  { value: "all", label: "All" },
  { value: "bible", label: "Bible" },
  { value: "music", label: "Music" },
  { value: "sermon", label: "Sermon" },
  { value: "service", label: "Service" },
  { value: "content", label: "Content" },
  { value: "correlation", label: "Correlations" },
];

export interface IntelligenceFeedProps {
  items: UnifiedIntelligenceItem[];
}

export function IntelligenceFeed({ items }: IntelligenceFeedProps) {
  const [filter, setFilter] = useState<UnifiedIntelligenceDomain | "all">("all");
  const visible = useMemo(
    () => (filter === "all" ? items : items.filter((item) => item.domain === filter)),
    [items, filter],
  );

  return (
    <section className="live-brain__panel workspace-feed">
      <h2>Intelligence Feed</h2>
      <div className="workspace-feed__filters" role="group" aria-label="Filter intelligence feed by domain">
        {FILTERS.map(({ value, label }) => (
          <button
            key={value}
            type="button"
            className={filter === value ? "workspace-feed__filter workspace-feed__filter--active" : "workspace-feed__filter"}
            aria-pressed={filter === value}
            onClick={() => setFilter(value)}
          >
            {label}
          </button>
        ))}
      </div>
      {visible.length === 0 ? (
        <p className="live-brain__hint">Nothing detected yet{filter !== "all" ? " for this domain" : ""}.</p>
      ) : (
        <ul className="workspace-card-list">
          {visible.map((item) => (
            <IntelligenceCard key={`${item.domain}-${item.id}`} item={item} busy={null} />
          ))}
        </ul>
      )}
    </section>
  );
}
