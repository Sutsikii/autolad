import { create } from "zustand";
import type { Point } from "./motion";

const IDLE_HIDE_MS = 4500;

interface AgentState {
  visible: boolean;
  label: string;
  failed: boolean;
  target: Point | null;
  /** Bumped per move / click so the cursor reacts even if the target is the same. */
  moveSeq: number;
  clickSeq: number;
  moveTo: (target: Point, label: string) => void;
  click: () => void;
  fail: (label: string) => void;
}

let hideTimer: ReturnType<typeof setTimeout> | undefined;

export const useAgentStore = create<AgentState>((set) => {
  const show = (patch: Partial<AgentState>) => {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => set({ visible: false }), IDLE_HIDE_MS);
    set({ visible: true, ...patch });
  };
  return {
    visible: false,
    label: "",
    failed: false,
    target: null,
    moveSeq: 0,
    clickSeq: 0,
    moveTo: (target, label) =>
      show({ target, label, failed: false, moveSeq: useAgentStore.getState().moveSeq + 1 }),
    click: () => show({ clickSeq: useAgentStore.getState().clickSeq + 1 }),
    fail: (label) => show({ label, failed: true }),
  };
});
