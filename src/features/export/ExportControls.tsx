import { useEffect, useState } from "react";
import { useTimelineStore } from "@/features/timeline/store";
import { btn, btnPrimary } from "@/shared/ui/styles";
import { useExportStore } from "./store";

const POLL_MS = 500;

export function ExportControls() {
  const [draft, setDraft] = useState(false);
  const job = useExportStore((s) => s.job);
  const starting = useExportStore((s) => s.starting);
  const start = useExportStore((s) => s.start);
  const refresh = useExportStore((s) => s.refresh);
  const cancel = useExportStore((s) => s.cancel);
  const hasClips = useTimelineStore((s) => s.cuts.length > 0);

  const running = job?.state === "running";
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  }, [running, refresh]);

  return (
    <div className="flex items-center gap-3">
      {job?.state === "running" && (
        <div className="flex items-center gap-2">
          <div className="h-1.5 w-40 overflow-hidden rounded bg-neutral-700">
            <div
              className="h-full bg-sky-500"
              style={{ width: `${Math.round(job.progress * 100)}%` }}
            />
          </div>
          <span className="w-9 text-right font-mono text-xs text-neutral-400">
            {Math.round(job.progress * 100)}%
          </span>
          <button className={btn} onClick={() => void cancel()}>
            Cancel
          </button>
        </div>
      )}
      <label className="flex items-center gap-1.5 text-xs text-neutral-400">
        <input
          type="checkbox"
          checked={draft}
          onChange={(e) => setDraft(e.target.checked)}
          className="accent-sky-500"
        />
        Draft
      </label>
      <button
        data-agent="export"
        className={btnPrimary}
        disabled={!hasClips || running || starting}
        onClick={() => void start(draft)}
      >
        Export
      </button>
    </div>
  );
}
