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
 *
 * Phase 6.2 (Operator Ergonomics): `confirmingKey` is forwarded straight
 * through to every `IntelligenceCard` so the one action currently armed
 * by `handleUnifiedAction`'s two-click confirm guard - today only the
 * Bible domain's "Display" action, the one action that immediately
 * projects content to a real live screen - can swap its own label to
 * "Confirm ...?" without this component needing to know which action
 * that is.
 *
 * Phase 6.8 (Operator Ergonomics): `editingId`/`editValue`/`onSaveEdit`/
 * `onCancelEdit`/`onEditValueChange` are forwarded straight through to
 * every `IntelligenceCard` - the exact same state Diagnostics Mode's
 * Pending Suggestions panel already uses, so editing a Bible reference
 * from either surface never desyncs. This component still never talks
 * to the backend itself; `LiveChurchBrain.tsx` owns the actual
 * `edit_suggestion` call.
 */
import { IntelligenceCard } from "./IntelligenceCard";
import { actionsFor, type UnifiedItemAction } from "./actions";
import { shortcutLegend } from "../../lib/attentionQueue";
import type { UnifiedIntelligenceItem } from "../../lib/unifiedFeed";

export interface AttentionQueueProps {
  items: UnifiedIntelligenceItem[];
  busy: string | null;
  onAction: (item: UnifiedIntelligenceItem, action: UnifiedItemAction) => void;
  /** Phase 6.2 - forwarded straight through to each `IntelligenceCard`;
   * see its own doc comment for what this key means. */
  confirmingKey?: string | null;
  /** Phase 6.8 (Operator Ergonomics: unified-queue Edit support) -
   * forwarded straight through to each `IntelligenceCard`; see its own
   * doc comment for what these mean. */
  editingId?: string | null;
  editValue?: string;
  onEditValueChange?: (value: string) => void;
  onSaveEdit?: (id: string) => void;
  onCancelEdit?: () => void;
}

export function AttentionQueue({
  items,
  busy,
  onAction,
  confirmingKey,
  editingId,
  editValue,
  onEditValueChange,
  onSaveEdit,
  onCancelEdit,
}: AttentionQueueProps) {
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
              confirmingKey={confirmingKey}
              editingId={editingId}
              editValue={editValue}
              onEditValueChange={onEditValueChange}
              onSaveEdit={onSaveEdit}
              onCancelEdit={onCancelEdit}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
