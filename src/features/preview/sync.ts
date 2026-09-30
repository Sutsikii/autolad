import type { CutSummary } from "@/ipc";
import { cutIndexAt } from "@/features/timeline/layout";

export interface Located {
  index: number;
  cut: CutSummary;
  /** Matching position in the source file, in seconds. */
  sourceTime: number;
}

/** If the video drifts further than this from the playhead, something else moved it: resync. */
export const DRIFT = 0.08;
const END_MARGIN = 0.04;

export function sourceTimeAt(cut: CutSummary, timelineTime: number): number {
  return cut.start + (timelineTime - cut.timeline_start);
}

export function playheadAt(cut: CutSummary, sourceTime: number): number {
  return cut.timeline_start + (sourceTime - cut.start);
}

/** The cut under `time`, or `null` once the playhead is at or past the end of the edit. */
export function locate(cuts: CutSummary[], time: number): Located | null {
  const index = cutIndexAt(cuts, time);
  const cut = index === null ? undefined : cuts[index];
  if (index === null || !cut) return null;
  return { index, cut, sourceTime: sourceTimeAt(cut, time) };
}

/** Like `locate`, but the very end of the edit shows the last frame of the last cut. */
export function locateForDisplay(cuts: CutSummary[], time: number): Located | null {
  const found = locate(cuts, time);
  if (found) return found;
  const index = cuts.length - 1;
  const last = cuts[index];
  return last ? { index, cut: last, sourceTime: last.end } : null;
}

export function reachedCutEnd(cut: CutSummary, sourceTime: number): boolean {
  return sourceTime >= cut.end - END_MARGIN;
}
