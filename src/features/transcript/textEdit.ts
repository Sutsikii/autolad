import { useLibraryStore } from "@/features/library/store";
import { useTimelineStore } from "@/features/timeline/store";
import { api, call, type AppError, type Result, type TextEditReport } from "@/ipc";
import { messageOf, notify } from "@/shared/notify";
import { useTranscriptStore } from "./store";

/**
 * Runs one text edit. The first one on a clip transcribes it word by word, which can take a
 * while, so the panel shows what is running.
 */
async function run(
  label: string,
  pending: () => Promise<Result<TextEditReport, AppError>>,
  describe: (report: TextEditReport) => string,
): Promise<void> {
  const store = useTranscriptStore.getState();
  if (store.editing) return;
  store.setEditing(label);
  try {
    const report = await call(pending());
    useTimelineStore.getState().setEdl(report.edl);
    notify.info(describe(report));
    // Word timings computed for this edit are now stored with the project.
    void store.loadStored(useLibraryStore.getState().assets);
  } catch (error) {
    notify.error(messageOf(error));
  } finally {
    useTranscriptStore.getState().setEditing(null);
  }
}

const seconds = (report: TextEditReport) => `${report.removed_seconds.toFixed(1)} s`;

export function cutPhrase(assetId: string, start: number, end: number, text: string) {
  return run(
    "Cutting the phrase",
    () => api.cutPhrase(assetId, start, end, text),
    (r) => `Phrase cut (${seconds(r)})`,
  );
}

export function removeFillers() {
  return run("Removing hesitations", () => api.removeFillers(), (r) =>
    r.removed.length === 0
      ? "No hesitation found"
      : `${r.removed.length} hesitation(s) removed (${seconds(r)})`,
  );
}

export function removeRetakes() {
  return run("Removing failed takes", () => api.removeRetakes(), (r) =>
    r.removed.length === 0
      ? "No failed take found"
      : `${r.removed.length} failed take(s) removed (${seconds(r)})`,
  );
}
