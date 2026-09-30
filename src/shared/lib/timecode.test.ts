import { describe, expect, it } from "vitest";
import { formatRuler, formatTimecode } from "./timecode";

describe("formatTimecode", () => {
  it("formats hours, minutes, seconds and frames", () => {
    expect(formatTimecode(0)).toBe("00:00:00:00");
    expect(formatTimecode(61.5)).toBe("00:01:01:15");
    expect(formatTimecode(3661)).toBe("01:01:01:00");
  });

  it("treats negative and non-finite values as zero", () => {
    expect(formatTimecode(-4)).toBe("00:00:00:00");
    expect(formatTimecode(Number.NaN)).toBe("00:00:00:00");
  });
});

describe("formatRuler", () => {
  it("uses m:ss and adds decimals for sub-second steps", () => {
    expect(formatRuler(75)).toBe("1:15");
    expect(formatRuler(2.5, 1)).toBe("0:02.5");
    expect(formatRuler(3725)).toBe("1:02:05");
  });
});
