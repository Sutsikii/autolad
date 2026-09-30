import { useEffect, useRef, useState } from "react";
import { useTimelineStore } from "@/features/timeline/store";
import { api, call } from "@/ipc";
import { messageOf } from "@/shared/notify";

const FRAME_WIDTH = 640;
const END_EPSILON = 0.001;

interface Wanted {
  time: number;
}

/**
 * Keeps the monitor on the frame under the playhead. One request at a time: while
 * scrubbing, intermediate positions are skipped and only the latest is fetched.
 * Only a fallback: once the proxy is ready the <video> shows the frame, so `enabled` is false.
 */
export function usePreviewFrame(enabled: boolean): { src: string | null; error: string | null } {
  const playhead = useTimelineStore((s) => s.playhead);
  const version = useTimelineStore((s) => s.version);
  const hasClips = useTimelineStore((s) => s.cuts.length > 0) && enabled;
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const wanted = useRef<Wanted | null>(null);
  const busy = useRef(false);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    if (!hasClips) {
      wanted.current = null;
      return;
    }
    const { duration } = useTimelineStore.getState();
    wanted.current = { time: Math.min(playhead, Math.max(0, duration - END_EPSILON)) };

    const pump = async () => {
      if (busy.current) return;
      busy.current = true;
      let request = wanted.current;
      while (request && alive.current) {
        try {
          const frame = await call(api.previewFrame(request.time, FRAME_WIDTH));
          if (alive.current) {
            setSrc(frame);
            setError(null);
          }
        } catch (e) {
          if (alive.current) setError(messageOf(e));
        }
        request = wanted.current === request ? null : wanted.current;
      }
      busy.current = false;
    };
    void pump();
  }, [playhead, version, hasClips]);

  // Derived, not reset in the effect: an emptied timeline must show the placeholder at once.
  return { src: hasClips ? src : null, error: hasClips ? error : null };
}
