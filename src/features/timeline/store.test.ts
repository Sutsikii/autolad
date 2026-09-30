import { beforeEach, describe, expect, it } from "vitest";
import type { EdlSummary } from "@/ipc";
import { MAX_ZOOM } from "./layout";
import { useTimelineStore } from "./store";

const edl = (durations: number[]): EdlSummary => {
  let at = 0;
  const cuts = durations.map((duration, index) => {
    const cut = { index, asset: "a", start: at, end: at + duration, duration, timeline_start: at };
    at += duration;
    return cut;
  });
  return { cuts, total_duration: at };
};

describe("timeline store", () => {
  beforeEach(() => useTimelineStore.getState().reset());

  it("keeps the playhead inside the edit", () => {
    const { setEdl, seek } = useTimelineStore.getState();
    setEdl(edl([2, 3]));
    seek(99);
    expect(useTimelineStore.getState().playhead).toBe(5);
    seek(-4);
    expect(useTimelineStore.getState().playhead).toBe(0);
  });

  it("pulls the playhead back when the edit shrinks", () => {
    const { setEdl, seek } = useTimelineStore.getState();
    setEdl(edl([10]));
    seek(8);
    setEdl(edl([3]));
    expect(useTimelineStore.getState().playhead).toBe(3);
  });

  it("drops a selection that no longer exists", () => {
    const { setEdl, select } = useTimelineStore.getState();
    setEdl(edl([1, 1, 1]));
    select(2);
    setEdl(edl([1]));
    expect(useTimelineStore.getState().selected).toBeNull();
  });

  it("fits the whole edit in the view and resets the scroll", () => {
    const { setEdl, setScroll, fit } = useTimelineStore.getState();
    setEdl(edl([10]));
    setScroll(500);
    fit(1000);
    const { pxPerSec, scrollX } = useTimelineStore.getState();
    expect(scrollX).toBe(0);
    expect(pxPerSec * 10).toBeLessThan(1000);
  });

  it("bumps the version on every edit and clamps the zoom", () => {
    const { setEdl, setZoom } = useTimelineStore.getState();
    setEdl(edl([1]));
    setEdl(edl([1]));
    expect(useTimelineStore.getState().version).toBe(2);
    setZoom(99999);
    expect(useTimelineStore.getState().pxPerSec).toBe(MAX_ZOOM);
  });
});
