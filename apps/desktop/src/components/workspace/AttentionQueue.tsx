/**
 * "What needs attention right now" (Phase 2.9 spec section 5B) - the
 * first actionable region of the Unified Operator Workspace. Every item
 * here already awaits a real operator decision
 * (`UnifiedIntelligenceItem.needsAttention`); accepting/rejecting/
 * acknowledging/dismissing one only ever calls the same existing command
 * the per-domain panels below already use (see `LiveChurchBrain.tsx`'s
 * `handleUnifiedAction`) - this component never talks to the backend
 * itself.
 *
 * Phase 6.1 (Operator Ergonomics): the heading also shows which keyboard
 * shortcuts (A/R) currently act on the top item and what they'll do
 * (`shortcutLegend`, `lib/attentionQueue.ts`), mirroring the
 * discoverability hint the Diagnostics-mode Pending Suggestions panel
 * already has - without this, an operator had no way to know the
 * shortcuts existed at all for this queue.
 */
import { IntelligenceCard } from "./IntelligenceCard";
import { actionsFor, type UnifiedItemAction } from "./actions";
import { shortcutLegend } from "../../lib/attentionQueue";
import type { UnifiedIntelligenceItem } from "../../lib/unifiedFeed";

export interface AttentionQueueProps {
  items: UnifiedIntelligenceItem[];
  busy: string | null;
  onAction: (item: UnifiedIntelligenceItem, action: UnifiedItemAction) => void;
}

export function AttentionQueue({ items, busy, onAction }: AttentionQueueProps) {
  const legend = shortcutLegend(items);
  return (
    <section className="live-brain__panel workspace-attention">
      <h2>
        Needs Attention {legend && <span className="live-brain__hint">({legend})</span>}
      </h2>
      {items.length === 0 ? (
        <p className="live-brain__hint">Nothing needs attention right now.</p>
      ) : (
        <ul className="workspace-card-list">
          {items.map((item) => (
            <IntelligenceCard
              key={`${item.domain}-${item.id}`}
              item={item}
              actions={actionsFor(item.domain)}
              busy={busy}
              onAction={onAction}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
