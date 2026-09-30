import { create } from "zustand";

export type SidebarTab = "auto-cut" | "transcript";

interface SidebarState {
  tab: SidebarTab;
  setTab: (tab: SidebarTab) => void;
}

export const useSidebarStore = create<SidebarState>((set) => ({
  tab: "auto-cut",
  setTab: (tab) => set({ tab }),
}));
