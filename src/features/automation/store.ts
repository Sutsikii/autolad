import { create } from "zustand";
import type { SilenceSettings } from "@/ipc";

export const DEFAULT_SETTINGS: SilenceSettings = { max_gap: 0.3, margin: 0.1, min_segment: 0.2 };

interface AutomationState {
  settings: SilenceSettings;
  update: (patch: Partial<SilenceSettings>) => void;
  reset: () => void;
}

export const useAutomationStore = create<AutomationState>((set) => ({
  settings: DEFAULT_SETTINGS,
  update: (patch) => set((s) => ({ settings: { ...s.settings, ...patch } })),
  reset: () => set({ settings: DEFAULT_SETTINGS }),
}));
