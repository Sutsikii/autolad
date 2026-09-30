import { describe, expect, it } from "vitest";
import { decodeBase64 } from "./mediaStore";

describe("decodeBase64", () => {
  it("decodes bytes, including the full 0..255 range", () => {
    expect(Array.from(decodeBase64("AAH//g=="))).toEqual([0, 1, 255, 254]);
  });

  it("decodes an empty string to no bytes", () => {
    expect(decodeBase64("").length).toBe(0);
  });
});
