import { describe, expect, it } from "vitest";
import type { AssetSummary } from "@/ipc";
import { DEFAULT_SEQUENCE, fitInside, sequenceSize } from "./frame";

describe("fitInside", () => {
  it("fills the width of a wide box with a landscape picture", () => {
    expect(fitInside({ width: 800, height: 600 }, 16 / 9)).toEqual({ width: 800, height: 450 });
  });

  it("is limited by the height for a portrait picture", () => {
    const fitted = fitInside({ width: 800, height: 600 }, 9 / 16);
    expect(fitted.height).toBe(600);
    expect(fitted.width).toBeCloseTo(337.5);
  });

  it("keeps the requested shape and never overflows", () => {
    for (const aspect of [0.4, 1, 1.78, 4]) {
      const box = { width: 500, height: 300 };
      const fitted = fitInside(box, aspect);
      expect(fitted.width).toBeLessThanOrEqual(box.width + 1e-9);
      expect(fitted.height).toBeLessThanOrEqual(box.height + 1e-9);
      expect(fitted.width / fitted.height).toBeCloseTo(aspect);
    }
  });

  it("gives an empty box for degenerate input", () => {
    const empty = { width: 0, height: 0 };
    expect(fitInside({ width: 0, height: 300 }, 1.5)).toEqual(empty);
    expect(fitInside({ width: 300, height: 300 }, 0)).toEqual(empty);
    expect(fitInside({ width: 300, height: 300 }, Number.NaN)).toEqual(empty);
  });
});

describe("sequenceSize", () => {
  const asset = (width: number | null, height: number | null): AssetSummary => ({
    id: "a",
    path: "a.mp4",
    duration: 1,
    has_audio: true,
    width,
    height,
    fps: 30,
  });

  it("follows the first clip, including portrait ones", () => {
    expect(sequenceSize(asset(1080, 1920))).toEqual({ width: 1080, height: 1920 });
  });

  it("falls back to widescreen when the size is unknown", () => {
    expect(sequenceSize(undefined)).toEqual(DEFAULT_SEQUENCE);
    expect(sequenceSize(asset(null, null))).toEqual(DEFAULT_SEQUENCE);
  });
});
