import { describe, expect, it } from "vitest";
import { cleanPath, fileName } from "./path";

describe("fileName", () => {
  it("handles both separators", () => {
    expect(fileName("C:\\videos\\take1.mov")).toBe("take1.mov");
    expect(fileName("/home/me/take1.mov")).toBe("take1.mov");
    expect(fileName("take1.mov")).toBe("take1.mov");
  });
});

describe("cleanPath", () => {
  it("strips surrounding quotes and whitespace", () => {
    expect(cleanPath('  "C:\\a b\\c.mp4" ')).toBe("C:\\a b\\c.mp4");
    expect(cleanPath("C:\\c.mp4")).toBe("C:\\c.mp4");
  });
});
