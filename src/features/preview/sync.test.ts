import { describe, expect, it } from "vitest";
import type { CutSummary } from "@/ipc";
import { locate, locateForDisplay, playheadAt, reachedCutEnd, sourceTimeAt } from "./sync";

const cut = (index: number, start: number, duration: number, at: number): CutSummary => ({
  index,
  asset: "a",
  start,
  end: start + duration,
  duration,
  timeline_start: at,
});

const CUTS = [cut(0, 10, 2, 0), cut(1, 30, 3, 2)];

describe("time mapping", () => {
  it("maps timeline time to source time and back", () => {
    const second = CUTS[1];
    if (!second) throw new Error("fixture");
    expect(sourceTimeAt(second, 3.5)).toBe(31.5);
    expect(playheadAt(second, 31.5)).toBe(3.5);
  });
});

describe("locate", () => {
  it("finds the cut and the source time under the playhead", () => {
    expect(locate(CUTS, 1.5)).toMatchObject({ index: 0, sourceTime: 11.5 });
    expect(locate(CUTS, 2)).toMatchObject({ index: 1, sourceTime: 30 });
  });

  it("is null at and past the end, or when empty", () => {
    expect(locate(CUTS, 5)).toBeNull();
    expect(locate([], 0)).toBeNull();
  });

  it("displays the last frame of the last cut at the end", () => {
    expect(locateForDisplay(CUTS, 5)).toMatchObject({ index: 1, sourceTime: 33 });
    expect(locateForDisplay([], 0)).toBeNull();
  });
});

describe("reachedCutEnd", () => {
  it("fires just before the cut end", () => {
    const first = CUTS[0];
    if (!first) throw new Error("fixture");
    expect(reachedCutEnd(first, 11.5)).toBe(false);
    expect(reachedCutEnd(first, 11.98)).toBe(true);
  });
});
