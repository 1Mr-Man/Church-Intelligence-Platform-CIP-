/**
 * The chronological, bounded, cross-domain feed (Phase 2.9 spec section
 * 5C) - Bible/Music/Sermon/Service/Content/Correlation activity in one
 * list, filterable by domain. Read-only: operator actions live in the
 * Attention Queue and the existing per-domain panels below, not here -
 * this region's job is visibility, not a second action surface.
 *
 * Phase 6.3 (Operator Ergonomics: feed search): also filterable by a free
 * text query (`searchIntelligenceFeed`, `lib/intelligenceFeed.ts`),
 * composed with the domain filter below (both narrow the same
 * already-fetched `items` array - no new fetch, no change to the
 * upstream 50-item cap).
 */
import { useMemo, useState } from "react";
import { IntelligenceCard } from "./IntelligenceCard";
import { searchIntelligenceFeed } from "../../lib/intelligenceFeed";
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
  const [query, setQuery] = useState("");
  const visible = useMemo(() => {
    const byDomain = filter === "all" ? items : items.filter((item) => item.domain === filter);
    return searchIntelligenceFeed(byDomain, query);
  }, [items, filter, query]);
  const isFiltered = filter !== "all" || query.trim() !== "";

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
      <input
        type="search"
        className="workspace-feed__search"
        placeholder="Search the feed..."
        aria-label="Search the intelligence feed"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      {visible.length === 0 ? (
        <p className="live-brain__hint">Nothing detected yet{isFiltered ? " matching this filter" : ""}.</p>
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
