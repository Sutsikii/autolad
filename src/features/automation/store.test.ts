import { beforeEach, describe, expect, it } from "vitest";
import { DEFAULT_SETTINGS, useAutomationStore } from "./store";

describe("automation store", () => {
  beforeEach(() => useAutomationStore.getState().reset());

  it("starts with the default settings", () => {
    expect(useAutomationStore.getState().settings).toEqual(DEFAULT_SETTINGS);
  });

  it("applies partial updates without touching other fields", () => {
    useAutomationStore.getState().update({ margin: 0.5 });
    expect(useAutomationStore.getState().settings).toEqual({ ...DEFAULT_SETTINGS, margin: 0.5 });
  });

  it("reset restores the defaults", () => {
    useAutomationStore.getState().update({ max_gap: 2 });
    useAutomationStore.getState().reset();
    expect(useAutomationStore.getState().settings).toEqual(DEFAULT_SETTINGS);
  });
});
