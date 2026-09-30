import { useRef } from "react";
import { useMediaStore } from "@/features/library/mediaStore";
import { nextCutStart, previousCutStart } from "@/features/timeline/layout";
import { useTimelineStore } from "@/features/timeline/store";
import { formatTimecode } from "@/shared/lib/timecode";
import { cn } from "@/shared/lib/utils";
import { Panel } from "@/shared/ui/Panel";
import { btn } from "@/shared/ui/styles";
import { usePreviewStore } from "./store";
import { locateForDisplay } from "./sync";
import { useVideoMonitor } from "./useVideoMonitor";
import { usePreviewFrame } from "./usePreviewFrame";

export function PreviewPanel() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const playing = usePreviewStore((s) => s.playing);
  const muted = usePreviewStore((s) => s.muted);
  const toggle = usePreviewStore((s) => s.toggle);
  const toggleMute = usePreviewStore((s) => s.toggleMute);

  const cuts = useTimelineStore((s) => s.cuts);
  const playhead = useTimelineStore((s) => s.playhead);
  const duration = useTimelineStore((s) => s.duration);
  const seek = useTimelineStore((s) => s.seek);
  const empty = cuts.length === 0;

  // The proxy plays smoothly with sound; until it is encoded, single PNG frames stand in.
  const assetId = locateForDisplay(cuts, playhead)?.cut.asset;
  const proxyReady = useMediaStore((s) => Boolean(assetId && s.byAsset[assetId]?.proxyUrl));
  const preparing = useMediaStore((s) => s.proxiesPending > 0);
  useVideoMonitor(videoRef);
  const { src, error } = usePreviewFrame(!proxyReady);

  return (
    <Panel title="Program" className="flex-1">
      <div className="relative flex min-h-0 flex-1 items-center justify-center bg-black">
        <video
          ref={videoRef}
          playsInline
          preload="auto"
          className={cn("max-h-full max-w-full object-contain", !proxyReady && "hidden")}
        />
        {!proxyReady && src && (
          <img src={src} alt="Program monitor" className="max-h-full max-w-full object-contain" />
        )}
        {!proxyReady && !src && (
          <p className="px-6 text-center text-xs leading-relaxed text-neutral-600">
            {empty
              ? "Import a clip, then double-click it or run Auto-cut to fill the timeline."
              : "Loading frame…"}
          </p>
        )}
        {preparing && !empty && !proxyReady && (
          <p className="absolute left-2 top-2 rounded bg-black/70 px-2 py-1 text-[11px] text-sky-400">
            Preparing smooth preview…
          </p>
        )}
        {error && (
          <p className="absolute bottom-2 left-2 right-2 truncate rounded bg-red-950/80 px-2 py-1 text-[11px] text-red-300">
            {error}
          </p>
        )}
      </div>

      <div className="flex h-11 shrink-0 items-center justify-between border-t border-black/60 px-3">
        <span className="w-28 font-mono text-sm text-sky-400">{formatTimecode(playhead)}</span>
        <div className="flex items-center gap-1.5">
          <button className={btn} disabled={empty} onClick={() => seek(0)} title="Go to start (Home)">
            ⏮
          </button>
          <button
            className={btn}
            disabled={empty}
            onClick={() => seek(previousCutStart(cuts, playhead))}
            title="Previous clip (↑)"
          >
            ◀◀
          </button>
          <button className={btn} disabled={empty} onClick={toggle} title="Play / pause (Space)">
            {playing ? "❚❚" : "▶"}
          </button>
          <button
            className={btn}
            disabled={empty}
            onClick={() => seek(nextCutStart(cuts, playhead, duration))}
            title="Next clip (↓)"
          >
            ▶▶
          </button>
          <button className={btn} disabled={empty} onClick={() => seek(duration)} title="Go to end (End)">
            ⏭
          </button>
          <button className={btn} onClick={toggleMute} title="Mute / unmute (M)">
            {muted ? "🔇" : "🔊"}
          </button>
        </div>
        <span className="w-28 text-right font-mono text-sm text-neutral-500">
          {formatTimecode(duration)}
        </span>
      </div>
    </Panel>
  );
}
