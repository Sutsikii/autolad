import { create } from "zustand";
import { api, call, type AssetSummary, type TranscriptReport } from "@/ipc";
import { messageOf, notify } from "@/shared/notify";

export const LANGUAGES = [
  ["auto", "Auto-detect"],
  ["fr", "French"],
  ["en", "English"],
  ["es", "Spanish"],
  ["de", "German"],
  ["it", "Italian"],
  ["pt", "Portuguese"],
  ["nl", "Dutch"],
] as const;

export const MODELS = [
  ["small", "Small · 190 MB"],
  ["tiny", "Tiny · 32 MB (drafts)"],
  ["base", "Base · 60 MB"],
  ["large-v3-turbo", "Large v3 turbo · 574 MB"],
] as const;

interface TranscriptState {
  byAsset: Record<string, TranscriptReport | undefined>;
  language: string;
  model: string;
  /** Asset being transcribed, and since when (ms), to show that something is happening. */
  running: { assetId: string; since: number } | null;
  /** Text edit in progress (cut, clean-up), shown while it runs. */
  editing: string | null;
  setEditing: (editing: string | null) => void;
  setLanguage: (language: string) => void;
  setModel: (model: string) => void;
  setReport: (report: TranscriptReport) => void;
  run: (asset: AssetSummary) => Promise<void>;
  /** Loads transcripts already stored in the project, without starting any job. */
  loadStored: (assets: AssetSummary[]) => Promise<void>;
}

export const useTranscriptStore = create<TranscriptState>((set, get) => ({
  byAsset: {},
  language: "auto",
  model: "small",
  running: null,
  editing: null,
  setEditing: (editing) => set({ editing }),
  setLanguage: (language) => set({ language }),
  setModel: (model) => set({ model }),
  setReport: (report) => set((s) => ({ byAsset: { ...s.byAsset, [report.asset]: report } })),
  run: async (asset) => {
    const { language, model, running, setReport } = get();
    if (running) return;
    set({ running: { assetId: asset.id, since: Date.now() } });
    try {
      const report = await call(api.transcribe(asset.id, language === "auto" ? null : language, model));
      setReport(report);
      notify.info(`Transcribed ${report.segments.length} phrases (${report.language})`);
    } catch (error) {
      notify.error(messageOf(error));
    } finally {
      set({ running: null });
    }
  },
  loadStored: async (assets) => {
    for (const asset of assets) {
      if (!asset.has_audio || get().byAsset[asset.id]) continue;
      try {
        const stored = await call(api.cachedTranscript(asset.id));
        if (stored) get().setReport(stored);
      } catch (error) {
        notify.error(messageOf(error));
      }
    }
  },
}));
