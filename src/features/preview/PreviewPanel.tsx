import { useRef } from "react";
import type { AssetSummary } from "@/ipc";
import { useMediaStore } from "@/features/library/mediaStore";
import { useLibraryStore } from "@/features/library/store";
import { nextCutStart, previousCutStart } from "@/features/timeline/layout";
import { useTimelineStore } from "@/features/timeline/store";
import { useElementSize } from "@/shared/hooks/useElementSize";
import { formatTimecode } from "@/shared/lib/timecode";
import { cn } from "@/shared/lib/utils";
import { Panel } from "@/shared/ui/Panel";
import { btn } from "@/shared/ui/styles";
import { fitInside, sequenceSize } from "./frame";
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

  // The frame has the shape of the sequence (its first clip, like an export); clips of another
  // shape get bars inside it, exactly as they will in the exported file.
  const firstAssetId = cuts[0]?.asset;
  const firstAsset = useLibraryStore((s) => s.assets.find((a) => a.id === firstAssetId));
  const sequence = sequenceSize(firstAsset);
  const [stageRef, stage] = useElementSize<HTMLDivElement>();
  const frame = fitInside(stage, sequence.width / sequence.height);

  // The proxy plays smoothly with sound; until it is encoded, single PNG frames stand in.
  const assetId = locateForDisplay(cuts, playhead)?.cut.asset;
  const proxyReady = useMediaStore((s) => Boolean(assetId && s.byAsset[assetId]?.proxyUrl));
  const preparing = useMediaStore((s) => s.proxiesPending > 0);
  useVideoMonitor(videoRef);
  const { src, error } = usePreviewFrame(!proxyReady);

  return (
    <Panel title="Program" actions={sequenceLabel(empty ? undefined : firstAsset)} className="flex-1">
      <div
        ref={stageRef}
        data-agent="monitor"
        className="relative flex min-h-0 flex-1 items-center justify-center bg-[#0d0d0d]"
      >
        <div
          data-testid="program-frame"
          className={cn("relative bg-black ring-1 ring-white/10", empty && "hidden")}
          style={{ width: frame.width, height: frame.height }}
        >
          <video
            ref={videoRef}
            playsInline
            preload="auto"
            className={cn("h-full w-full object-contain", !proxyReady && "hidden")}
          />
          {!proxyReady && src && (
            <img src={src} alt="Program monitor" className="h-full w-full object-contain" />
          )}
        </div>
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

/** "1080×1920 · 30 fps": what the export will be, next to the monitor. */
function sequenceLabel(asset: AssetSummary | undefined) {
  if (!asset?.width || !asset.height) return undefined;
  const fps = asset.fps ? ` · ${Number(asset.fps.toFixed(2))} fps` : "";
  return (
    <span className="font-mono text-[11px] text-neutral-500" title="Size of the exported video">
      {asset.width}×{asset.height}
      {fps}
    </span>
  );
}
