import { create } from "zustand";
import { api, call, type JobStatus } from "@/ipc";
import { fileName } from "@/shared/lib/path";
import { messageOf, notify } from "@/shared/notify";

interface ExportState {
  job: JobStatus | null;
  starting: boolean;
  start: (draft: boolean, output: string | null) => Promise<void>;
  refresh: () => Promise<void>;
  cancel: () => Promise<void>;
}

const MEGABYTE = 1024 * 1024;

export const useExportStore = create<ExportState>((set, get) => ({
  job: null,
  starting: false,
  start: async (draft, output) => {
    set({ starting: true });
    try {
      const id = await call(api.renderStart(draft, output));
      set({ job: await call(api.renderStatus(id)) });
    } catch (error) {
      notify.error(messageOf(error));
    } finally {
      set({ starting: false });
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
