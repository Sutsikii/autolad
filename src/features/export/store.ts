import { create } from "zustand";
import { api, call, type JobStatus } from "@/ipc";
import { fileName } from "@/shared/lib/path";
import { messageOf, notify } from "@/shared/notify";

interface ExportState {
  job: JobStatus | null;
  starting: boolean;
  /** Level the loudness to -14 LUFS, like streaming platforms do. */
  normalize: boolean;
  setNormalize: (normalize: boolean) => void;
  /** Burn captions of the speech into the video. */
  captions: boolean;
  setCaptions: (captions: boolean) => void;
  /** A subtitle file is being written (the first time, clips are transcribed word by word). */
  writingSubtitles: boolean;
  exportSubtitles: (output: string) => Promise<void>;
  start: (draft: boolean, output: string | null) => Promise<void>;
  /** Follows a render started elsewhere (by an agent), as if it had been started here. */
  track: (jobId: string) => Promise<void>;
  refresh: () => Promise<void>;
  cancel: () => Promise<void>;
}

const MEGABYTE = 1024 * 1024;

export const useExportStore = create<ExportState>((set, get) => ({
  job: null,
  starting: false,
  normalize: true,
  setNormalize: (normalize) => set({ normalize }),
  captions: false,
  setCaptions: (captions) => set({ captions }),
  writingSubtitles: false,
  exportSubtitles: async (output) => {
    set({ writingSubtitles: true });
    notify.info("Writing subtitles…");
    try {
      const done = await call(api.exportSubtitles(output));
      notify.info(`Wrote ${done.cues} subtitles to ${fileName(done.path)}`);
    } catch (error) {
      notify.error(messageOf(error));
    } finally {
      set({ writingSubtitles: false });
    }
  },
  start: async (draft, output) => {
    set({ starting: true });
    try {
      const { normalize, captions } = get();
      if (captions) notify.info("Preparing captions…");
      const id = await call(api.renderStart(draft, output, normalize, captions));
      set({ job: await call(api.renderStatus(id)) });
    } catch (error) {
      notify.error(messageOf(error));
    } finally {
      set({ starting: false });
    }
  },
  track: async (jobId) => {
    try {
      set({ job: await call(api.renderStatus(jobId)) });
    } catch (error) {
      notify.error(messageOf(error));
    }
  },
  refresh: async () => {
    const current = get().job;
    if (current?.state !== "running") return;
    try {
      const next = await call(api.renderStatus(current.id));
      set({ job: next });
      if (next.state === "done") {
        const size = (next.size_bytes / MEGABYTE).toFixed(1);
        notify.info(`Exported ${fileName(next.output)} (${size} MB)`);
      } else if (next.state === "failed") {
        notify.error(`Export failed: ${next.error}`);
      }
    } catch (error) {
      notify.error(messageOf(error));
    }
  },
  cancel: async () => {
    const current = get().job;
    if (current?.state !== "running") return;
    try {
      set({ job: await call(api.renderCancel(current.id)) });
      notify.info("Export cancelled");
    } catch (error) {
      notify.error(messageOf(error));
    }
  },
}));
