import { nextCutStart, previousCutStart } from "@/features/timeline/layout";
import { useTimelineStore } from "@/features/timeline/store";
import { formatTimecode } from "@/shared/lib/timecode";
import { Panel } from "@/shared/ui/Panel";
import { btn } from "@/shared/ui/styles";
import { usePreviewStore } from "./store";
import { usePlayback } from "./usePlayback";
import { usePreviewFrame } from "./usePreviewFrame";

export function PreviewPanel() {
  usePlayback();
  const { src, error } = usePreviewFrame();
  const playing = usePreviewStore((s) => s.playing);
  const toggle = usePreviewStore((s) => s.toggle);

  const cuts = useTimelineStore((s) => s.cuts);
  const playhead = useTimelineStore((s) => s.playhead);
  const duration = useTimelineStore((s) => s.duration);
  const seek = useTimelineStore((s) => s.seek);
  const empty = cuts.length === 0;

  return (
    <Panel title="Program" className="flex-1">
      <div className="relative flex min-h-0 flex-1 items-center justify-center bg-black">
        {src ? (
          <img src={src} alt="Program monitor" className="max-h-full max-w-full object-contain" />
        ) : (
          <p className="px-6 text-center text-xs leading-relaxed text-neutral-600">
            {empty
              ? "Import a clip, then double-click it or run Auto-cut to fill the timeline."
              : "Loading frame…"}
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
        </div>
        <span className="w-28 text-right font-mono text-sm text-neutral-500">
          {formatTimecode(duration)}
        </span>
      </div>
    </Panel>
  );
}
