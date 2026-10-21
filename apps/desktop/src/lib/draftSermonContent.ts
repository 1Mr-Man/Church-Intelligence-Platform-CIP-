import type { ContentCandidate, TranscriptSegment } from "../domain";

export interface SermonDraftPack {
  notes: string;
  slides: string[];
  devotional: string;
  newsletter: string;
}

function clean(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/**
 * Builds review-only content drafts from facts already captured by CIP.
 * This is intentionally deterministic and local: it never invents Bible
 * references or claims that a generated paragraph was approved theology.
 */
export function buildSermonDraftPack(
  title: string,
  transcript: TranscriptSegment[],
  savedContent: ContentCandidate[],
): SermonDraftPack {
  const excerpts = transcript.map((segment) => clean(segment.text)).filter(Boolean).slice(0, 12);
  const concepts = savedContent.map((candidate) => clean(candidate.workingConcept)).filter(Boolean).slice(0, 8);
  const highlights = concepts.length > 0 ? concepts : excerpts.slice(0, 5);
  const summary = highlights.length > 0 ? highlights.join(" ") : "No accepted sermon findings are available yet.";

  return {
    notes: `DRAFT SERMON NOTES\n\n${title}\n\nMain observations:\n${highlights.map((item) => `- ${item}`).join("\n") || "- Continue reviewing the transcript."}\n\nSource transcript excerpts:\n${excerpts.map((item) => `- ${item}`).join("\n") || "- None captured."}`,
    slides: [
      title,
      ...highlights.slice(0, 6),
      "Response / application",
      "Closing reflection",
    ],
    devotional: `DRAFT DEVOTIONAL\n\n${title}\n\nReflection\n${summary}\n\nPrayer prompt\nHow should this message shape our response this week?\n\nNote: review wording and references before publishing.`,
    newsletter: `DRAFT NEWSLETTER\n\nThis week's message: ${title}\n\n${summary}\n\nSuggested next step\nDiscuss one practical response with your household or small group.\n\nNote: review wording, names, and references before sending.`,
  };
}
