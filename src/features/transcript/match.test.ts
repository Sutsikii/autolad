import { describe, expect, it } from "vitest";
import type { CutSummary } from "@/ipc";
import { activePhraseIndex, formatStamp, isOnTimeline, timelinePositionOf } from "./match";

const cut = (asset: string, start: number, duration: number, at: number): CutSummary => ({
  index: 0,
  asset,
  start,
  end: start + duration,
  duration,
  timeline_start: at,
});

describe("activePhraseIndex", () => {
  const phrases = [
    { start: 0, end: 2 },
    { start: 3, end: 5 },
  ];

  it("finds the phrase being spoken and treats the end as exclusive", () => {
    expect(activePhraseIndex(phrases, 0)).toBe(0);
    expect(activePhraseIndex(phrases, 1.9)).toBe(0);
    expect(activePhraseIndex(phrases, 3)).toBe(1);
  });

  it("is null in a pause, before the start and after the end", () => {
    expect(activePhraseIndex(phrases, 2.5)).toBeNull();
    expect(activePhraseIndex(phrases, -1)).toBeNull();
    expect(activePhraseIndex(phrases, 5)).toBeNull();
    expect(activePhraseIndex([], 1)).toBeNull();
  });
});

describe("timelinePositionOf", () => {
  const cuts = [cut("a", 10, 4, 0), cut("b", 0, 3, 4), cut("a", 30, 5, 7)];

  it("maps a source moment to the timeline through the clip that covers it", () => {
    expect(timelinePositionOf(cuts, "a", 11)).toBe(1);
    expect(timelinePositionOf(cuts, "b", 2)).toBe(6);
    expect(timelinePositionOf(cuts, "a", 32.5)).toBe(9.5);
  });

  it("is null when the moment was cut out or the source is absent", () => {
    expect(timelinePositionOf(cuts, "a", 20)).toBeNull();
    expect(timelinePositionOf(cuts, "a", 14)).toBeNull();
    expect(timelinePositionOf(cuts, "c", 1)).toBeNull();
  });
});

describe("formatStamp", () => {
  it("formats minutes and hours", () => {
    expect(formatStamp(0)).toBe("0:00");
    expect(formatStamp(75.9)).toBe("1:15");
    expect(formatStamp(3725)).toBe("1:02:05");
  });

  it("copes with negative and non-finite values", () => {
    expect(formatStamp(-3)).toBe("0:00");
    expect(formatStamp(Number.NaN)).toBe("0:00");
  });
});

describe("isOnTimeline", () => {
  const cuts = [cut("a", 10, 5, 0)];

  it("is true while any part of the phrase is played", () => {
    expect(isOnTimeline(cuts, "a", { start: 9, end: 11 })).toBe(true);
    expect(isOnTimeline(cuts, "a", { start: 12, end: 13 })).toBe(true);
  });

  it("is false once the phrase is cut out or belongs to another source", () => {
    expect(isOnTimeline(cuts, "a", { start: 15, end: 16 })).toBe(false);
    expect(isOnTimeline(cuts, "b", { start: 11, end: 12 })).toBe(false);
  });
});
