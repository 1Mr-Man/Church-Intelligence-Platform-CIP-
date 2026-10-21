import type { PresentationItem } from "../../domain";
import type { UnifiedIntelligenceItem } from "../../lib/unifiedFeed";

interface NowNextWhyProps {
  activeDisplayItem: PresentationItem | null;
  preparedItems: PresentationItem[];
  attentionItems: UnifiedIntelligenceItem[];
}

function presentationLabel(item: PresentationItem | null): string {
  if (!item) return "Nothing is live";
  return item.content.type === "scripture" ? item.content.reference : item.content.type;
}

/** A compact operator mental model: what is live, what is queued, and why CIP
 * is asking for attention. It is read-only and never changes the display. */
export function NowNextWhy({ activeDisplayItem, preparedItems, attentionItems }: NowNextWhyProps) {
  const next = preparedItems.find((item) => item.id !== activeDisplayItem?.id) ?? null;
  const why = attentionItems[0];
  return (
    <section className="now-next-why" aria-label="Now next why">
      <div>
        <span>Now</span>
        <strong>{presentationLabel(activeDisplayItem)}</strong>
      </div>
      <div>
        <span>Next</span>
        <strong>{presentationLabel(next)}</strong>
      </div>
      <div>
        <span>Why</span>
        <strong>{why ? why.summary : "No operator decision needed"}</strong>
      </div>
    </section>
  );
}
