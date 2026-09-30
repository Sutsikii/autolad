import { convertFileSrc } from "@tauri-apps/api/core";
import { create } from "zustand";
import { api, call } from "@/ipc";
import { messageOf, notify } from "@/shared/notify";

export interface ThumbnailTiles {
  image: HTMLImageElement;
  /** Seconds between two tiles: tile `i` shows the frame at `i * step`. */
  step: number;
  tiles: number;
  tileWidth: number;
  tileHeight: number;
}

/** What the editor shows instead of the source file. Each part appears as soon as it is ready. */
export interface AssetMedia {
  proxyUrl?: string;
  thumbnails?: ThumbnailTiles;
  /** One byte (0..255) per `1 / peaksPerSecond` seconds. */
  peaks?: Uint8Array;
  peaksPerSecond?: number;
}

interface MediaState {
  byAsset: Record<string, AssetMedia>;
  /** Assets whose proxy is still being encoded. */
  proxiesPending: number;
  prepare: (assetId: string) => Promise<void>;
}

const started = new Set<string>();

export function decodeBase64(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("the thumbnail strip could not be loaded"));
    image.src = url;
  });
}

export const useMediaStore = create<MediaState>((set) => {
  const patch = (assetId: string, part: AssetMedia) =>
    set((s) => ({ byAsset: { ...s.byAsset, [assetId]: { ...s.byAsset[assetId], ...part } } }));
  // Losing thumbnails or a waveform must not stop editing: report and carry on.
  const report = (what: string) => (error: unknown) =>
    notify.error(`${what}: ${messageOf(error)}`);

  const proxy = async (assetId: string) => {
    set((s) => ({ proxiesPending: s.proxiesPending + 1 }));
    try {
      const path = await call(api.prepareProxy(assetId));
      patch(assetId, { proxyUrl: convertFileSrc(path) });
    } finally {
      set((s) => ({ proxiesPending: s.proxiesPending - 1 }));
    }
  };

  const thumbnails = async (assetId: string) => {
    const strip = await call(api.prepareThumbnails(assetId));
    const image = await loadImage(convertFileSrc(strip.path));
    patch(assetId, {
      thumbnails: {
        image,
        step: strip.step,
        tiles: strip.tiles,
        tileWidth: strip.tile_width,
        tileHeight: strip.tile_height,
      },
    });
  };

  const waveform = async (assetId: string) => {
    const wave = await call(api.prepareWaveform(assetId));
    patch(assetId, { peaks: decodeBase64(wave.base64), peaksPerSecond: wave.peaks_per_second });
  };

  return {
    byAsset: {},
    proxiesPending: 0,
    prepare: async (assetId) => {
      if (started.has(assetId)) return;
      started.add(assetId);
      await Promise.all([
        proxy(assetId).catch(report("Preview")),
        thumbnails(assetId).catch(report("Thumbnails")),
        waveform(assetId).catch(report("Waveform")),
      ]);
    },
  };
});
