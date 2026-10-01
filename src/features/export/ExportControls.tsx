import { useEffect, useState } from "react";
import {
  chooseExportPath,
  chooseFcpxmlPath,
  chooseSubtitlesPath,
} from "@/features/project/actions";
import { api, call } from "@/ipc";
import { fileName } from "@/shared/lib/path";
import { useTimelineStore } from "@/features/timeline/store";
import { messageOf, notify } from "@/shared/notify";
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
  const normalize = useExportStore((s) => s.normalize);
  const setNormalize = useExportStore((s) => s.setNormalize);
  const captions = useExportStore((s) => s.captions);
  const setCaptions = useExportStore((s) => s.setCaptions);
  const writingSubtitles = useExportStore((s) => s.writingSubtitles);
  const exportSubtitles = useExportStore((s) => s.exportSubtitles);
  const hasClips = useTimelineStore((s) => s.cuts.length > 0);

  const running = job?.state === "running";

  const begin = async () => {
    try {
      const output = await chooseExportPath(draft);
      if (output) await start(draft, output);
    } catch (error) {
      notify.error(messageOf(error));
    }
  };
  const writeSubtitles = async () => {
    try {
      const output = await chooseSubtitlesPath();
      if (output) await exportSubtitles(output);
    } catch (error) {
      notify.error(messageOf(error));
    }
  };
  const writeFcpxml = async () => {
    try {
      const output = await chooseFcpxmlPath();
      if (!output) return;
      const saved = await call(api.exportFcpxml(output));
      notify.info(`Exported ${fileName(saved)} for Final Cut Pro / DaVinci Resolve`);
    } catch (error) {
      notify.error(messageOf(error));
    }
  };
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
      <label
        className="flex items-center gap-1.5 text-xs text-neutral-400"
        title="Level the sound to -14 LUFS, the loudness YouTube and Spotify play at"
      >
        <input
          type="checkbox"
          checked={normalize}
          onChange={(e) => setNormalize(e.target.checked)}
          className="accent-sky-500"
        />
        Loudness
      </label>
      <label
        className="flex items-center gap-1.5 text-xs text-neutral-400"
        title="Burn captions of the speech into the video"
      >
        <input
          type="checkbox"
          checked={captions}
          onChange={(e) => setCaptions(e.target.checked)}
          className="accent-sky-500"
        />
        Captions
      </label>
      <button
        className={btn}
        disabled={!hasClips || writingSubtitles}
        onClick={() => void writeSubtitles()}
        title="Save the subtitles of the edit as .srt or .vtt"
      >
        Subtitles…
      </button>
      <button
        className={btn}
        disabled={!hasClips}
        onClick={() => void writeFcpxml()}
        title="Save the edit as Final Cut Pro XML, to finish it in Final Cut Pro or DaVinci Resolve"
      >
        FCPXML…
      </button>
      <button
        data-agent="export"
        className={btnPrimary}
        disabled={!hasClips || running || starting}
        onClick={() => void begin()}
      >
        Export
      </button>
    </div>
  );
}
