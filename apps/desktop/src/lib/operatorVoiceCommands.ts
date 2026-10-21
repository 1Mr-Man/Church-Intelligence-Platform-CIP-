import type { UnifiedItemAction } from "../components/workspace/actions";

export type OperatorVoiceCommand =
  | { kind: "top-item-action"; action: UnifiedItemAction; phrase: string }
  | { kind: "open-diagnostics"; phrase: string }
  | { kind: "focus-search"; phrase: string };

/**
 * Conservative command grammar for the optional speech-to-command layer.
 * It only maps phrases to actions already visible on the top Attention Queue
 * item; unknown or ambiguous phrases return null and are never executed.
 */
export function parseOperatorVoiceCommand(
  input: string,
  availableActions: UnifiedItemAction[],
): OperatorVoiceCommand | null {
  const phrase = input.trim().replace(/\s+/g, " ");
  const normalized = phrase.toLowerCase().replace(/[.,!?]/g, "");
  if (!normalized) return null;

  if (/^(open|show) diagnostics(?: mode)?$/.test(normalized)) {
    return { kind: "open-diagnostics", phrase };
  }
  if (/^(focus|open) search$/.test(normalized)) {
    return { kind: "focus-search", phrase };
  }

  const action: UnifiedItemAction | null =
    /^(approve|accept|confirm|display|show) (the )?(next|top|first)( item| suggestion)?$/.test(normalized)
      ? availableActions.includes("display")
        ? "display"
        : availableActions.includes("approve")
          ? "approve"
          : availableActions.includes("accept")
            ? "accept"
            : null
      : /^(reject|decline|dismiss) (the )?(next|top|first)( item| suggestion)?$/.test(normalized)
        ? availableActions.includes("reject")
          ? "reject"
          : availableActions.includes("dismiss")
            ? "dismiss"
            : null
        : /^(review|edit) (the )?(next|top|first)( item| suggestion)?$/.test(normalized)
          ? availableActions.includes("review")
            ? "review"
            : availableActions.includes("edit")
              ? "edit"
              : null
          : null;

  return action ? { kind: "top-item-action", action, phrase } : null;
}
