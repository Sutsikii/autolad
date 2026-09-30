import { useEffect } from "react";
import { useTimelineStore } from "@/features/timeline/store";
import { usePreviewStore } from "./store";

/** Advances the playhead in real time; the monitor shows the latest frame it managed to fetch. */
export function usePlayback(): void {
  const playing = usePreviewStore((s) => s.playing);

  useEffect(() => {
    if (!playing) return;
    const { duration, playhead, seek } = useTimelineStore.getState();
    if (duration <= 0) {
      usePreviewStore.getState().setPlaying(false);
      return;
    }
    if (playhead >= duration) seek(0);

    let frame = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const state = useTimelineStore.getState();
      const next = state.playhead + (now - last) / 1000;
      last = now;
      if (next >= state.duration) {
        state.seek(state.duration);
        usePreviewStore.getState().setPlaying(false);
        return;
      }
      state.seek(next);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playing]);
}
