import type { AssetSummary } from "@/ipc";

export interface Size {
  width: number;
  height: number;
}

/** Shape assumed until the first clip says otherwise. */
export const DEFAULT_SEQUENCE: Size = { width: 1920, height: 1080 };

/** Largest `aspect`-shaped box that fits inside `box`. */
export function fitInside(box: Size, aspect: number): Size {
  if (box.width <= 0 || box.height <= 0 || !Number.isFinite(aspect) || aspect <= 0) {
    return { width: 0, height: 0 };
  }
  const widthLimited = box.width / aspect <= box.height;
  return widthLimited
    ? { width: box.width, height: box.width / aspect }
    : { width: box.height * aspect, height: box.height };
}

/**
 * Size of the sequence: what an export would be, i.e. the first clip's source.
 * Clips of another shape are letterboxed inside it, as they are at render time.
 */
export function sequenceSize(firstAsset: AssetSummary | undefined): Size {
  const width = firstAsset?.width;
  const height = firstAsset?.height;
  return width && height ? { width, height } : DEFAULT_SEQUENCE;
}
