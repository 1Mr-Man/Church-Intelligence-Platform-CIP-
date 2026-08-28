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
}

export function IntelligenceCard({ item, actions, busy, onAction }: IntelligenceCardProps) {
  const confidencePercent = Math.round(item.confidence.score * 100);
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
      {actions && actions.length > 0 && onAction && (
        <div className="workspace-card__actions">
          {actions.map((action) => (
            <button
              key={action}
              type="button"
              disabled={busy === `${item.domain}-${action}-${item.id}`}
              onClick={() => onAction(item, action)}
            >
              {ACTION_LABELS[action]}
            </button>
          ))}
        </div>
      )}
    </li>
  );
}
