import { create } from "zustand";

interface ProjectState {
  /** Project file the work is saved to (and kept in sync with), if any. */
  file: string | null;
  setFile: (file: string | null) => void;
}

export const useProjectStore = create<ProjectState>((set) => ({
  file: null,
  setFile: (file) => set({ file }),
}));
