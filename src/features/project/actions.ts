import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { refreshProject } from "@/app/refreshProject";
import { useLibraryStore } from "@/features/library/store";
import { usePreviewStore } from "@/features/preview/store";
import { useTimelineStore } from "@/features/timeline/store";
import { api, call } from "@/ipc";
import { fileName, stem, withExtension } from "@/shared/lib/path";
import { messageOf, notify } from "@/shared/notify";
import { useProjectStore } from "./store";

export const PROJECT_EXTENSION = "autolad";
const PROJECT_FILTER = { name: "AutoLad project", extensions: [PROJECT_EXTENSION] };
const VIDEO_FILTER = {
  name: "Video",
  extensions: ["mp4", "mov", "mkv", "avi", "webm", "m4v", "mts", "ts", "flv", "wmv"],
};

/** Name offered in "Save as": the project file, else the first rush. */
function suggestedName(): string {
  const { file } = useProjectStore.getState();
  if (file) return stem(file);
  const first = useLibraryStore.getState().assets[0];
  return first ? stem(first.path) : "Untitled";
}

async function run(action: () => Promise<void>): Promise<void> {
  try {
    await action();
  } catch (error) {
    notify.error(messageOf(error));
  }
}

export function saveProjectAs(): Promise<void> {
  return run(async () => {
    const chosen = await save({
      defaultPath: `${suggestedName()}.${PROJECT_EXTENSION}`,
      filters: [PROJECT_FILTER],
    });
    if (!chosen) return;
    const path = await call(api.saveProject(withExtension(chosen, PROJECT_EXTENSION)));
    useProjectStore.getState().setFile(path);
    notify.info(`Saved ${fileName(path)}`);
  });
}

/** Saves to the bound file, or asks where when the project has none yet. */
export function saveProject(): Promise<void> {
  const { file } = useProjectStore.getState();
  if (!file) return saveProjectAs();
  return run(async () => {
    const path = await call(api.saveProject(file));
    notify.info(`Saved ${fileName(path)}`);
  });
}

function resetEditor(): void {
  usePreviewStore.getState().setPlaying(false);
  useLibraryStore.getState().reset();
  useTimelineStore.getState().reset();
  useProjectStore.getState().setFile(null);
}

/** An unbound project with work in it would be lost: ask first. */
async function confirmDiscard(): Promise<boolean> {
  const hasWork = useLibraryStore.getState().assets.length > 0;
  if (!hasWork || useProjectStore.getState().file) return true;
  return ask("This project has not been saved. Discard it?", {
    title: "AutoLad",
    kind: "warning",
    okLabel: "Discard",
    cancelLabel: "Cancel",
  });
}

export function newProject(): Promise<void> {
  return run(async () => {
    if (!(await confirmDiscard())) return;
    await call(api.newProject());
    resetEditor();
    notify.info("New project");
  });
}

export function openProject(): Promise<void> {
  return run(async () => {
    const chosen = await open({ multiple: false, filters: [PROJECT_FILTER] });
    if (typeof chosen !== "string") return;
    if (!(await confirmDiscard())) return;
    const report = await call(api.openProject(chosen));
    usePreviewStore.getState().setPlaying(false);
    await refreshProject();
    useTimelineStore.getState().seek(0);
    useTimelineStore.getState().select(null);
    if (report.missing_files.length > 0) {
      const names = report.missing_files.map(fileName).join(", ");
      notify.error(`Source files not found: ${names}`);
    } else {
      notify.info(`Opened ${fileName(chosen)}`);
    }
  });
}

/** Native file picker; the chosen videos are handed to `onPath` one at a time. */
export function pickVideos(onPath: (path: string) => Promise<void>): Promise<void> {
  return run(async () => {
    const chosen = await open({ multiple: true, filters: [VIDEO_FILTER] });
    const paths = Array.isArray(chosen) ? chosen : chosen ? [chosen] : [];
    for (const path of paths) await onPath(path);
  });
}

/** Where to write the subtitles (.srt or .vtt); `null` when the user cancels. */
export async function chooseSubtitlesPath(): Promise<string | null> {
  const chosen = await save({
    defaultPath: `${suggestedName()}.srt`,
    filters: [
      { name: "SubRip subtitles", extensions: ["srt"] },
      { name: "WebVTT subtitles", extensions: ["vtt"] },
    ],
  });
  if (!chosen) return null;
  return /\.(srt|vtt)$/i.test(chosen) ? chosen : withExtension(chosen, "srt");
}

/** Where to write the export; `null` when the user cancels. */
export async function chooseExportPath(draft: boolean): Promise<string | null> {
  const suffix = draft ? "_draft" : "";
  const chosen = await save({
    defaultPath: `${suggestedName()}${suffix}.mp4`,
    filters: [{ name: "MP4 video", extensions: ["mp4"] }],
  });
  return chosen ? withExtension(chosen, "mp4") : null;
}
