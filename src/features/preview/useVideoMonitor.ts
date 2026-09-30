import { useEffect, type RefObject } from "react";
import { useMediaStore } from "@/features/library/mediaStore";
import { useTimelineStore } from "@/features/timeline/store";
import { notify } from "@/shared/notify";
import { usePreviewStore } from "./store";
import { DRIFT, locate, locateForDisplay, playheadAt, reachedCutEnd } from "./sync";
import { showSource } from "./videoSource";

const proxyOf = (assetId: string) => useMediaStore.getState().byAsset[assetId]?.proxyUrl;

/**
 * Drives the <video> of the program monitor from the timeline.
 * Paused: the video follows the playhead (scrubbing, edits). Playing: the video is the clock
 * and the playhead follows it, hopping to the next cut's source position at each cut end.
 */
export function useVideoMonitor(videoRef: RefObject<HTMLVideoElement | null>): void {
  const playing = usePreviewStore((s) => s.playing);
  const muted = usePreviewStore((s) => s.muted);
  const playhead = useTimelineStore((s) => s.playhead);
  const version = useTimelineStore((s) => s.version);
  const byAsset = useMediaStore((s) => s.byAsset);

  useEffect(() => {
    const video = videoRef.current;
    if (video) video.muted = muted;
  }, [muted, videoRef]);

  // Paused: show the frame under the playhead.
  useEffect(() => {
    const video = videoRef.current;
    if (!video || playing) return;
    const found = locateForDisplay(useTimelineStore.getState().cuts, playhead);
    const url = found ? proxyOf(found.cut.asset) : undefined;
    if (found && url) void showSource(video, url, found.sourceTime);
  }, [playhead, version, byAsset, playing, videoRef]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    if (!playing) {
      video.pause();
      return;
    }

    const stop = () => usePreviewStore.getState().setPlaying(false);
    const timeline = useTimelineStore.getState();
    if (timeline.playhead >= timeline.duration) timeline.seek(0);

    let frame = 0;
    let cancelled = false;
    let resyncing = false;

    const tick = async () => {
      if (cancelled) return;
      const { cuts, playhead: head, seek } = useTimelineStore.getState();
      const found = locate(cuts, head);
      const url = found ? proxyOf(found.cut.asset) : undefined;
      if (!found || !url) {
        if (found) notify.info("The preview of this clip is still being prepared");
        stop();
        return;
      }

      const offSource = video.dataset.src !== url;
      const drifted = Math.abs(video.currentTime - found.sourceTime) > DRIFT;
      if ((offSource || drifted) && !resyncing) {
        resyncing = true;
        video.pause();
        await showSource(video, url, found.sourceTime);
        resyncing = false;
        if (cancelled) return;
        await video.play().catch(stop);
      } else if (!resyncing) {
        // The video ended early (its file can be a hair shorter than the probed duration).
        const atEnd = reachedCutEnd(found.cut, video.currentTime) || video.ended;
        // Stepping a hair past the cut lands the playhead in the next one.
        const cutEnd = found.cut.timeline_start + found.cut.duration + 0.001;
        seek(atEnd ? cutEnd : playheadAt(found.cut, video.currentTime));
      }
      frame = requestAnimationFrame(() => void tick());
    };

    void video.play().catch(stop);
    frame = requestAnimationFrame(() => void tick());
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      video.pause();
    };
  }, [playing, videoRef]);
}
