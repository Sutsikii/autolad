import { create } from "zustand";
import type { CutSummary, EdlSummary } from "@/ipc";
import { clamp, DEFAULT_ZOOM, fitZoom, MAX_ZOOM, MIN_ZOOM } from "./layout";

interface TimelineState {
  cuts: CutSummary[];
  duration: number;
  playhead: number;
  selected: number | null;
  pxPerSec: number;
  scrollX: number;
  /** Bumped on every edit so the preview refetches even if the playhead did not move. */
  version: number;
  setEdl: (edl: EdlSummary) => void;
  seek: (time: number) => void;
  select: (index: number | null) => void;
  setZoom: (pxPerSec: number) => void;
  setScroll: (scrollX: number) => void;
  /** Zooms so the whole edit fits a view of `viewWidth` px, scrolled back to the start. */
  fit: (viewWidth: number) => void;
  reset: () => void;
}

const INITIAL = {
  cuts: [],
  duration: 0,
  playhead: 0,
  selected: null,
  pxPerSec: DEFAULT_ZOOM,
  scrollX: 0,
  version: 0,
};

export const useTimelineStore = create<TimelineState>((set) => ({
  ...INITIAL,
  setEdl: (edl) =>
    set((s) => ({
      cuts: edl.cuts,
      duration: edl.total_duration,
      playhead: clamp(s.playhead, 0, edl.total_duration),
      selected: s.selected !== null && s.selected < edl.cuts.length ? s.selected : null,
      version: s.version + 1,
    })),
  seek: (time) => set((s) => ({ playhead: clamp(time, 0, s.duration) })),
  select: (index) => set({ selected: index }),
  setZoom: (pxPerSec) => set({ pxPerSec: clamp(pxPerSec, MIN_ZOOM, MAX_ZOOM) }),
  setScroll: (scrollX) => set({ scrollX: Math.max(0, scrollX) }),
  fit: (viewWidth) => set((s) => ({ pxPerSec: fitZoom(s.duration, viewWidth), scrollX: 0 })),
  reset: () => set(INITIAL),
}));
