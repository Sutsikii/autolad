import { describe, expect, it } from "vitest";
import { cleanPath, fileName, stem, withExtension } from "./path";

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

describe("stem", () => {
  it("drops the last extension only", () => {
    expect(stem("C:\\v\\take1.mov")).toBe("take1");
    expect(stem("a.b.mp4")).toBe("a.b");
    expect(stem("noext")).toBe("noext");
    expect(stem(".hidden")).toBe(".hidden");
  });
});

describe("withExtension", () => {
  it("adds the extension once, case-insensitively", () => {
    expect(withExtension("C:\\p\\cut", "autolad")).toBe("C:\\p\\cut.autolad");
    expect(withExtension("cut.autolad", "autolad")).toBe("cut.autolad");
    expect(withExtension("cut.AUTOLAD", "autolad")).toBe("cut.AUTOLAD");
  });
});
