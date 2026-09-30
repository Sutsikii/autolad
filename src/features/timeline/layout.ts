import type { CutSummary } from "@/ipc";

/** Width of the track-label column, in CSS px. */
export const GUTTER = 44;
export const RULER_H = 24;
export const END_PADDING = 32;

export const MIN_ZOOM = 2;
export const MAX_ZOOM = 400;
export const DEFAULT_ZOOM = 40;

const RULER_STEPS = [0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1800, 3600];

export const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value));

export function timeToX(time: number, pxPerSec: number, scrollX: number): number {
  return GUTTER + time * pxPerSec - scrollX;
}

export function xToTime(x: number, pxPerSec: number, scrollX: number): number {
  return Math.max(0, (x - GUTTER + scrollX) / pxPerSec);
}

/** Index of the cut under `time`, or `null` past the end of the edit. */
export function cutIndexAt(cuts: CutSummary[], time: number): number | null {
  const index = cuts.findIndex(
    (cut) => time >= cut.timeline_start && time < cut.timeline_start + cut.duration,
  );
  return index === -1 ? null : index;
}

/** Smallest ruler interval that keeps labels at least `minPx` apart. */
export function rulerStep(pxPerSec: number, minPx = 90): number {
  return RULER_STEPS.find((step) => step * pxPerSec >= minPx) ?? 3600;
}

export function fitZoom(duration: number, viewWidth: number): number {
  const usable = viewWidth - GUTTER - END_PADDING;
  if (duration <= 0 || usable <= 0) return DEFAULT_ZOOM;
  return clamp(usable / duration, MIN_ZOOM, MAX_ZOOM);
}

export function maxScroll(duration: number, pxPerSec: number, viewWidth: number): number {
  return Math.max(0, duration * pxPerSec + END_PADDING - (viewWidth - GUTTER));
}

/** Start of the cut the playhead is in, or of the previous one if it already sits on it. */
export function previousCutStart(cuts: CutSummary[], time: number): number {
  const starts = cuts.map((cut) => cut.timeline_start).filter((start) => start < time - 0.05);
  return starts.length > 0 ? Math.max(...starts) : 0;
}

/** Start of the next cut, or the end of the edit when there is none. */
export function nextCutStart(cuts: CutSummary[], time: number, duration: number): number {
  const starts = cuts.map((cut) => cut.timeline_start).filter((start) => start > time + 0.05);
  return starts.length > 0 ? Math.min(...starts) : duration;
}
