import { create } from "zustand";

interface PreviewState {
  playing: boolean;
  setPlaying: (playing: boolean) => void;
  toggle: () => void;
}

export const usePreviewStore = create<PreviewState>((set) => ({
  playing: false,
  setPlaying: (playing) => set({ playing }),
  toggle: () => set((s) => ({ playing: !s.playing })),
}));
