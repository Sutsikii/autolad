import type { CutSummary } from "@/ipc";

export interface Phrase {
  start: number;
  end: number;
}

/** Index of the phrase being spoken at `sourceTime`, or `null` in a pause or out of range. */
export function activePhraseIndex(phrases: Phrase[], sourceTime: number): number | null {
  const index = phrases.findIndex((p) => sourceTime >= p.start && sourceTime < p.end);
  return index === -1 ? null : index;
}

/**
 * Where a moment of a source file sits on the timeline, or `null` when no clip of that source
 * covers it (it was cut out, or the source is not on the timeline).
 */
export function timelinePositionOf(
  cuts: CutSummary[],
  assetId: string,
  sourceTime: number,
): number | null {
  const cut = cuts.find((c) => c.asset === assetId && sourceTime >= c.start && sourceTime < c.end);
  return cut ? cut.timeline_start + (sourceTime - cut.start) : null;
}

/** Whether any part of a source phrase is still played by the timeline. */
export function isOnTimeline(cuts: CutSummary[], assetId: string, phrase: Phrase): boolean {
  return cuts.some((c) => c.asset === assetId && c.start < phrase.end && c.end > phrase.start);
}

/** `m:ss`, or `h:mm:ss` past an hour. */
export function formatStamp(seconds: number): string {
  const total = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
