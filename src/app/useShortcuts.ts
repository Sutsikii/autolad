import { useEffect } from "react";
import { usePreviewStore } from "@/features/preview/store";
import {
  newProject,
  openProject,
  saveProject,
  saveProjectAs,
} from "@/features/project/actions";
import {
  deleteSelected,
  moveSelected,
  splitAtPlayhead,
  stepHistory,
} from "@/features/timeline/actions";
import { nextCutStart, previousCutStart } from "@/features/timeline/layout";
import { useTimelineStore } from "@/features/timeline/store";
import { TIMELINE_FPS } from "@/shared/lib/timecode";

const FRAME = 1 / TIMELINE_FPS;

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))
  );
}

function handleKey(event: KeyboardEvent): boolean {
  const { seek, playhead, cuts, duration } = useTimelineStore.getState();
  const step = event.shiftKey ? 1 : FRAME;

  switch (event.code) {
    case "Space":
      usePreviewStore.getState().toggle();
      return true;
    case "KeyM":
      usePreviewStore.getState().toggleMute();
      return true;
    case "KeyS":
      void splitAtPlayhead();
      return true;
    case "Delete":
    case "Backspace":
      void deleteSelected();
      return true;
    case "Home":
      seek(0);
      return true;
    case "End":
      seek(duration);
      return true;
    case "ArrowUp":
      seek(previousCutStart(cuts, playhead));
      return true;
    case "ArrowDown":
      seek(nextCutStart(cuts, playhead, duration));
      return true;
    case "ArrowLeft":
      if (event.altKey) void moveSelected(-1);
      else seek(playhead - step);
      return true;
    case "ArrowRight":
      if (event.altKey) void moveSelected(1);
      else seek(playhead + step);
      return true;
    default:
      return false;
  }
}

/** File and history shortcuts (Ctrl+N/O/S, Ctrl+Z/Y). */
function handleCtrlKey(event: KeyboardEvent): boolean {
  switch (event.code) {
    case "KeyZ":
      void stepHistory(event.shiftKey ? "redo" : "undo");
      return true;
    case "KeyY":
      void stepHistory("redo");
      return true;
    case "KeyN":
      void newProject();
      return true;
    case "KeyO":
      void openProject();
      return true;
    case "KeyS":
      void (event.shiftKey ? saveProjectAs() : saveProject());
      return true;
    default:
      return false;
  }
}

export function useShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey || event.metaKey) {
        // Text fields keep their own undo.
        const textUndo = isTyping(event.target) && ["KeyZ", "KeyY"].includes(event.code);
        if (!textUndo && handleCtrlKey(event)) event.preventDefault();
        return;
      }
      if (isTyping(event.target)) return;
      if (handleKey(event)) event.preventDefault();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
