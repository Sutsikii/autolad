import { create } from "zustand";

interface PreviewState {
  playing: boolean;
  muted: boolean;
  setPlaying: (playing: boolean) => void;
  toggle: () => void;
  toggleMute: () => void;
}

export const usePreviewStore = create<PreviewState>((set) => ({
  playing: false,
  muted: false,
  setPlaying: (playing) => set({ playing }),
  toggle: () => set((s) => ({ playing: !s.playing })),
  toggleMute: () => set((s) => ({ muted: !s.muted })),
}));
