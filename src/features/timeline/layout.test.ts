import { describe, expect, it } from "vitest";
import type { CutSummary } from "@/ipc";
import {
  cutIndexAt,
  DEFAULT_ZOOM,
  fitZoom,
  GUTTER,
  maxScroll,
  nextCutStart,
  previousCutStart,
  rulerStep,
  timeToX,
  xToTime,
} from "./layout";

const cut = (index: number, start: number, duration: number, at: number): CutSummary => ({
  index,
  asset: "a",
  start,
  end: start + duration,
  duration,
  timeline_start: at,
});

const CUTS = [cut(0, 10, 2, 0), cut(1, 30, 3, 2)];

describe("coordinates", () => {
  it("round-trips time and x", () => {
    const x = timeToX(4, 50, 20);
    expect(x).toBe(GUTTER + 4 * 50 - 20);
    expect(xToTime(x, 50, 20)).toBeCloseTo(4);
  });

  it("never maps left of the origin to a negative time", () => {
    expect(xToTime(0, 50, 0)).toBe(0);
  });
});

describe("cutIndexAt", () => {
  it("finds the cut under a time and treats boundaries as the next cut", () => {
    expect(cutIndexAt(CUTS, 0)).toBe(0);
    expect(cutIndexAt(CUTS, 1.99)).toBe(0);
    expect(cutIndexAt(CUTS, 2)).toBe(1);
    expect(cutIndexAt(CUTS, 5)).toBeNull();
    expect(cutIndexAt([], 0)).toBeNull();
  });
});

describe("cut navigation", () => {
  it("jumps to the previous and next cut starts", () => {
    expect(previousCutStart(CUTS, 3)).toBe(2);
    expect(previousCutStart(CUTS, 2)).toBe(0);
    expect(previousCutStart(CUTS, 0)).toBe(0);
    expect(nextCutStart(CUTS, 0, 5)).toBe(2);
    expect(nextCutStart(CUTS, 3, 5)).toBe(5);
  });
});

describe("zoom", () => {
  it("picks a ruler step that keeps labels apart", () => {
    expect(rulerStep(100)).toBe(1);
    expect(rulerStep(10)).toBe(10);
    expect(rulerStep(0.001)).toBe(3600);
  });

  it("fits the edit into the view and falls back when empty", () => {
    expect(fitZoom(0, 1000)).toBe(DEFAULT_ZOOM);
    const zoom = fitZoom(10, 1000);
    expect(timeToX(10, zoom, 0)).toBeLessThan(1000);
  });

  it("only scrolls when the edit overflows the view", () => {
    expect(maxScroll(10, 10, 1000)).toBe(0);
    expect(maxScroll(100, 50, 1000)).toBeGreaterThan(0);
  });
});
