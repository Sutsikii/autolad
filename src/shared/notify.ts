import { create } from "zustand";

interface Notice {
  kind: "info" | "error";
  text: string;
}

interface NoticeState {
  notice: Notice | null;
  info: (text: string) => void;
  error: (text: string) => void;
}

/** Last message shown in the status bar; features report through it instead of owning UI. */
export const useNotice = create<NoticeState>((set) => ({
  notice: null,
  info: (text) => set({ notice: { kind: "info", text } }),
  error: (text) => set({ notice: { kind: "error", text } }),
}));

export const notify = {
  info: (text: string) => useNotice.getState().info(text),
  error: (text: string) => useNotice.getState().error(text),
};

export function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
