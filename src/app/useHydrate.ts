import { useEffect } from "react";
import { useMediaStore } from "@/features/library/mediaStore";
import { useLibraryStore } from "@/features/library/store";
import { useTimelineStore } from "@/features/timeline/store";
import { api, call } from "@/ipc";
import { messageOf, notify } from "@/shared/notify";

/** The backend keeps the project: a webview reload must show it again, not an empty editor. */
export function useHydrate(): void {
  useEffect(() => {
    call(api.projectStatus()).then(
      (status) => {
        useLibraryStore.getState().setAssets(status.assets);
        for (const asset of status.assets) void useMediaStore.getState().prepare(asset.id);
        useTimelineStore.getState().setEdl(status.edl);
      },
      (error: unknown) => notify.error(messageOf(error)),
    );
  }, []);
}
