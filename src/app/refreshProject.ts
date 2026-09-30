import { useMediaStore } from "@/features/library/mediaStore";
import { useLibraryStore } from "@/features/library/store";
import { useProjectStore } from "@/features/project/store";
import { useTimelineStore } from "@/features/timeline/store";
import { useTranscriptStore } from "@/features/transcript/store";
import { api, call } from "@/ipc";
import { messageOf, notify } from "@/shared/notify";

/** Shows what the backend currently holds (startup, or after an agent changed the project). */
export async function refreshProject(): Promise<void> {
  try {
    const status = await call(api.projectStatus());
    useLibraryStore.getState().setAssets(status.assets);
    for (const asset of status.assets) void useMediaStore.getState().prepare(asset);
    useTimelineStore.getState().setEdl(status.edl);
    void useTranscriptStore.getState().loadStored(status.assets);
    useProjectStore.getState().setFile(status.project_file);
  } catch (error) {
    notify.error(messageOf(error));
  }
}
