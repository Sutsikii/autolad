import { GUTTER, RULER_H, timeToX } from "@/features/timeline/layout";
import { TRACKS } from "@/features/timeline/draw";
import { useTimelineStore } from "@/features/timeline/store";
import type { Point } from "./motion";

/** What the backend tells us about an action; enough to find where it happens on screen. */
export interface ActionSite {
  tool: string;
  index: number | null;
  time: number | null;
}

function centerOf(name: string): Point | null {
  const element = document.querySelector(`[data-agent="${name}"]`);
  if (!element) return null;
  const box = element.getBoundingClientRect();
  return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
}

function timelineCanvas(): DOMRect | null {
  const canvas = document.querySelector('[data-agent="timeline"] canvas');
  return canvas ? canvas.getBoundingClientRect() : null;
}

const clampX = (box: DOMRect, x: number) => Math.min(box.right - 8, Math.max(box.left + GUTTER + 4, x));

/** Middle of clip `index` on the video track (or the end of the edit when it does not exist yet). */
function clipPoint(index: number): Point | null {
  const box = timelineCanvas();
  if (!box) return null;
  const { cuts, duration, pxPerSec, scrollX } = useTimelineStore.getState();
  const cut = cuts[index];
  const time = cut ? cut.timeline_start + cut.duration / 2 : duration;
  return {
    x: clampX(box, box.left + timeToX(time, pxPerSec, scrollX)),
    y: box.top + TRACKS.video.y + TRACKS.video.h / 2,
  };
}

/** A spot on the ruler, where a person would click to look at that moment. */
function rulerPoint(time: number): Point | null {
  const box = timelineCanvas();
  if (!box) return null;
  const { pxPerSec, scrollX } = useTimelineStore.getState();
  return { x: clampX(box, box.left + timeToX(time, pxPerSec, scrollX)), y: box.top + RULER_H / 2 };
}

/** Where on screen an agent action takes place, or `null` when the UI has nothing for it. */
export function resolveTarget(site: ActionSite): Point | null {
  switch (site.tool) {
    case "import_media":
      return centerOf("import");
    case "detect_silences":
    case "build_silence_edl":
      return centerOf("auto-cut");
    case "transcribe":
      return centerOf("transcript") ?? centerOf("auto-cut");
    case "edit_edl":
      return (site.index === null ? null : clipPoint(site.index)) ?? centerOf("timeline");
    case "cut_text":
    case "remove_fillers":
    case "remove_retakes":
      return centerOf("clean-up") ?? centerOf("transcript");
    case "undo":
    case "redo":
      return centerOf(site.tool);
    case "preview_frame":
      return (site.time === null ? null : rulerPoint(site.time)) ?? centerOf("monitor");
    case "save_project":
    case "open_project":
      return centerOf("project-title");
    case "render_start":
    case "render_cancel":
      return centerOf("export");
    default:
      return null;
  }
}
