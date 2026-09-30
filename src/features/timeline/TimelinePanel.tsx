import { useState } from "react";
import { Panel } from "@/shared/ui/Panel";
import { btn } from "@/shared/ui/styles";
import { formatTimecode } from "@/shared/lib/timecode";
import { deleteSelected, moveSelected, splitAtPlayhead } from "./actions";
import { MAX_ZOOM, MIN_ZOOM } from "./layout";
import { useTimelineStore } from "./store";
import { TimelineCanvas } from "./TimelineCanvas";

export function TimelinePanel() {
  const [fitRequest, setFitRequest] = useState(0);
  const pxPerSec = useTimelineStore((s) => s.pxPerSec);
  const setZoom = useTimelineStore((s) => s.setZoom);
  const duration = useTimelineStore((s) => s.duration);
  const clipCount = useTimelineStore((s) => s.cuts.length);
  const selected = useTimelineStore((s) => s.selected);

  const toolbar = (
    <div className="flex items-center gap-1.5">
      <button className={btn} onClick={() => void splitAtPlayhead()} title="Split at playhead (S)">
        Split
      </button>
      <button
        className={btn}
        disabled={selected === null}
        onClick={() => void deleteSelected()}
        title="Ripple delete (Delete)"
      >
        Delete
      </button>
      <button
        className={btn}
        disabled={selected === null}
        onClick={() => void moveSelected(-1)}
        title="Move clip left (Alt+←)"
      >
        ◀
      </button>
      <button
        className={btn}
        disabled={selected === null}
        onClick={() => void moveSelected(1)}
        title="Move clip right (Alt+→)"
      >
        ▶
      </button>
      <span className="mx-1 h-4 w-px bg-neutral-700" />
      <button className={btn} onClick={() => setFitRequest((n) => n + 1)} title="Fit to window">
        Fit
      </button>
      <input
        type="range"
        min={Math.log(MIN_ZOOM)}
        max={Math.log(MAX_ZOOM)}
        step={0.01}
        value={Math.log(pxPerSec)}
        onChange={(e) => setZoom(Math.exp(Number(e.target.value)))}
        className="w-28 accent-sky-500"
        aria-label="Timeline zoom"
      />
    </div>
  );

  return (
    <Panel
      title={`Timeline — ${clipCount} clip${clipCount === 1 ? "" : "s"} · ${formatTimecode(duration)}`}
      actions={toolbar}
      agent="timeline"
      className="h-60"
    >
      <TimelineCanvas fitRequest={fitRequest} />
    </Panel>
  );
}
