/**
 * One `UnifiedIntelligenceItem`, rendered consistently regardless of which
 * domain produced it (Phase 2.9). Never flattens the item into an
 * unexplained colored card (spec rule 8): domain, confidence, assertion
 * level, and status are always plain text labels, never color-only
 * signals (spec section 13/rule 5/6).
 */
import type { UnifiedIntelligenceItem } from "../../lib/unifiedFeed";
import { ACTION_LABELS, type UnifiedItemAction } from "./actions";

const DOMAIN_LABELS: Record<UnifiedIntelligenceItem["domain"], string> = {
  bible: "Bible",
  music: "Music",
  sermon: "Sermon",
  service: "Service",
  content: "Content",
  correlation: "Correlation",
};

export interface IntelligenceCardProps {
  item: UnifiedIntelligenceItem;
  actions?: UnifiedItemAction[];
  busy: string | null;
  onAction?: (item: UnifiedIntelligenceItem, action: UnifiedItemAction) => void;
  /** Phase 6.2 (Operator Ergonomics: Display confirmation/undo) - the
   * `${domain}-${action}-${id}` key of an action currently armed by
   * `confirmGuard`'s two-click guard (see `LiveChurchBrain.tsx`'s
   * `handleUnifiedAction`), if any. That button alone swaps its label to
   * "Confirm {label}?" so the operator sees the guard is live before their
   * next click actually fires it - never a blocking dialog, since only
   * the one armed button changes. */
  confirmingKey?: string | null;
  /** Phase 6.8 (Operator Ergonomics: unified-queue Edit support) - the
   * exact same `editingId`/`editValue` state and `edit_suggestion`-backed
   * save/cancel handlers Diagnostics Mode's Pending Suggestions panel
   * already uses (see `LiveChurchBrain.tsx`), so editing the same
   * `Suggestion` from either surface never desyncs. Undefined/omitted on
   * any card whose domain has no "edit" action - `actionsFor` never
   * offers it outside `bible`, so these stay unused there. */
  editingId?: string | null;
  editValue?: string;
  onEditValueChange?: (value: string) => void;
  onSaveEdit?: (id: string) => void;
  onCancelEdit?: () => void;
}

export function IntelligenceCard({
  item,
  actions,
  busy,
  onAction,
  confirmingKey,
  editingId,
  editValue,
  onEditValueChange,
  onSaveEdit,
  onCancelEdit,
}: IntelligenceCardProps) {
  const confidencePercent = Math.round(item.confidence.score * 100);
  const isEditing = editingId === item.id;
  return (
    <li className={`workspace-card workspace-card--${item.domain}`}>
      <div className="workspace-card__header">
        <span className={`workspace-card__domain workspace-card__domain--${item.domain}`}>
          {DOMAIN_LABELS[item.domain]}
        </span>
        <strong>{item.summary}</strong>
        <span className="workspace-card__confidence">{confidencePercent}%</span>
      </div>
      <p className="workspace-card__meta">
        {item.assertionLevel.toUpperCase()} &middot; {item.rawStatus.toUpperCase()}
        {item.evidenceCount > 0 && <> &middot; {item.evidenceCount} evidence</>}
      </p>
      {item.detailLine && <p className="workspace-card__detail">{item.detailLine}</p>}
      {isEditing ? (
        <div className="workspace-card__edit">
          <input
            value={editValue ?? ""}
            onChange={(e) => onEditValueChange?.(e.target.value)}
            aria-label="Edit reference"
          />
          <button type="button" disabled={busy === `edit-${item.id}`} onClick={() => onSaveEdit?.(item.id)}>
            Save
          </button>
          <button type="button" onClick={() => onCancelEdit?.()}>
            Cancel
          </button>
        </div>
      ) : (
        actions &&
        actions.length > 0 &&
        onAction && (
          <div className="workspace-card__actions">
            {actions.map((action) => {
              const key = `${item.domain}-${action}-${item.id}`;
              const isConfirming = confirmingKey === key;
              return (
                <button
                  key={action}
                  type="button"
                  className={isConfirming ? "workspace-card__action--confirming" : undefined}
                  disabled={busy === key}
                  onClick={() => onAction(item, action)}
                >
                  {isConfirming ? `Confirm ${ACTION_LABELS[action]}?` : ACTION_LABELS[action]}
                </button>
              );
            })}
          </div>
        )
      )}
    </li>
  );
}
