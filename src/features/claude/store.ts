import { create } from "zustand";
import { api, call, type ClaudeApp, type ClaudeSetup } from "@/ipc";
import { messageOf, notify } from "@/shared/notify";

interface ClaudeState {
  setup: ClaudeSetup | null;
  /** App being connected. */
  connecting: ClaudeApp | null;
  load: () => Promise<void>;
  connect: (app: ClaudeApp) => Promise<void>;
}

const DONE: Record<ClaudeApp, string> = {
  desktop: "AutoLad added to Claude Desktop: restart Claude Desktop to use it",
  code: "AutoLad added to Claude Code: new sessions can use it (/mcp in a running one)",
};

export const useClaudeStore = create<ClaudeState>((set, get) => ({
  setup: null,
  connecting: null,
  load: async () => {
    try {
      set({ setup: await call(api.claudeSetup()) });
    } catch (error) {
      notify.error(messageOf(error));
    }
  },
  connect: async (app) => {
    if (get().connecting) return;
    set({ connecting: app });
    try {
      set({ setup: await call(api.connectClaude(app)) });
      notify.info(DONE[app]);
    } catch (error) {
      notify.error(messageOf(error));
    } finally {
      set({ connecting: null });
    }
  },
}));
