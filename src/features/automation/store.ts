import { create } from "zustand";
import type { SilenceSettings } from "@/ipc";

export const DEFAULT_SETTINGS: SilenceSettings = { max_gap: 0.3, margin: 0.1, min_segment: 0.2 };
export const DEFAULT_NOISE_DB = -30;

interface AutomationState {
  settings: SilenceSettings;
  /** Level below which audio counts as silence (dBFS). */
  noiseDb: number;
  running: boolean;
  update: (patch: Partial<SilenceSettings>) => void;
  setNoiseDb: (noiseDb: number) => void;
  setRunning: (running: boolean) => void;
  reset: () => void;
}

export const useAutomationStore = create<AutomationState>((set) => ({
  settings: DEFAULT_SETTINGS,
  noiseDb: DEFAULT_NOISE_DB,
  running: false,
  update: (patch) => set((s) => ({ settings: { ...s.settings, ...patch } })),
  setNoiseDb: (noiseDb) => set({ noiseDb }),
  setRunning: (running) => set({ running }),
  reset: () => set({ settings: DEFAULT_SETTINGS, noiseDb: DEFAULT_NOISE_DB }),
}));
