import { useEffect } from "react";
import { refreshProject } from "@/app/refreshProject";
import { useSidebarStore } from "@/app/sidebarStore";
import { useLibraryStore } from "@/features/library/store";
import { useTranscriptStore } from "@/features/transcript/store";
import { useTimelineStore } from "@/features/timeline/store";
import { events } from "@/ipc";
import { useAgentStore } from "./store";
import { resolveTarget } from "./targets";

/**
 * Follows an AI agent: the cursor glides to where each action happens, clicks when it is done,
 * and the UI reloads the project it changed. The work itself already ran in the backend.
 */
export function useAgentEvents(): void {
  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;

    try {
      void events.agentActivity
        .listen(({ payload }) => {
          const agent = useAgentStore.getState();
          if (payload.phase === "started") {
            // The transcript tab must be visible for the cursor to have something to go to.
            if (payload.tool === "transcribe") useSidebarStore.getState().setTab("transcript");
            const target = resolveTarget(payload);
            if (target) agent.moveTo(target, payload.label);
            return;
          }
          if (payload.phase === "failed") {
            agent.fail(`${payload.label} — failed`);
            return;
          }
          agent.click();
          if (payload.changes_project) void refreshProject();
          if (payload.tool === "transcribe") {
            void useTranscriptStore.getState().loadStored(useLibraryStore.getState().assets);
          }
          if (payload.tool === "preview_frame" && payload.time !== null) {
            useTimelineStore.getState().seek(payload.time);
          }
        })
        .then((unlisten) => {
          if (cancelled) unlisten();
          else stop = unlisten;
        });
    } catch {
      // Plain browser (vite preview, tests): no Tauri event bus, so nothing to follow.
    }

    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);
}
