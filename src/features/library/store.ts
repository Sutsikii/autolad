import { create } from "zustand";
import { api, call, type AssetSummary } from "@/ipc";
import { cleanPath, fileName } from "@/shared/lib/path";
import { useMediaStore } from "./mediaStore";
import { messageOf, notify } from "@/shared/notify";

interface LibraryState {
  assets: AssetSummary[];
  selectedId: string | null;
  importing: number;
  setAssets: (assets: AssetSummary[]) => void;
  reset: () => void;
  select: (id: string | null) => void;
  importPath: (path: string) => Promise<void>;
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  assets: [],
  selectedId: null,
  importing: 0,
  setAssets: (assets) =>
    set((s) => ({
      assets,
      selectedId: assets.some((a) => a.id === s.selectedId) ? s.selectedId : (assets[0]?.id ?? null),
    })),
  reset: () => set({ assets: [], selectedId: null }),
  select: (id) => set({ selectedId: id }),
  importPath: async (raw) => {
    const path = cleanPath(raw);
    if (!path) return;
    set((s) => ({ importing: s.importing + 1 }));
    try {
      const asset = await call(api.importMedia(path));
      const known = get().assets.some((a) => a.id === asset.id);
      set((s) => ({ assets: known ? s.assets : [...s.assets, asset], selectedId: asset.id }));
      void useMediaStore.getState().prepare(asset);
      notify.info(known ? `${fileName(path)} is already imported` : `Imported ${fileName(path)}`);
    } catch (error) {
      notify.error(messageOf(error));
    } finally {
      set((s) => ({ importing: s.importing - 1 }));
    }
  },
}));
