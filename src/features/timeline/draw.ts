import type { AssetMedia, ThumbnailTiles } from "@/features/library/mediaStore";
import type { CutSummary } from "@/ipc";
import { formatRuler } from "@/shared/lib/timecode";
import { clamp, GUTTER, RULER_H, rulerStep, timeToX } from "./layout";

export interface DrawState {
  width: number;
  height: number;
  cuts: CutSummary[];
  duration: number;
  pxPerSec: number;
  scrollX: number;
  playhead: number;
  selected: number | null;
  /** Thumbnails and waveforms per asset id; clips draw flat colour until theirs arrive. */
  media: Record<string, AssetMedia | undefined>;
}

const COLORS = {
  background: "#1b1b1b",
  ruler: "#232323",
  rulerTick: "#5a5a5a",
  rulerText: "#9a9a9a",
  gutter: "#232323",
  trackLine: "#2c2c2c",
  video: "#3b6fa8",
  videoSelected: "#5b95d1",
  audio: "#3f8f6b",
  audioSelected: "#5bbd93",
  audioMuted: "#2f2f2f",
  mutedText: "#8a8a8a",
  wave: "#bdeed6",
  waveSelected: "#ffffff",
  selectedTint: "rgba(120, 180, 255, 0.28)",
  labelBackdrop: "rgba(0, 0, 0, 0.55)",
  clipText: "#f0f0f0",
  selection: "#ffffff",
  playhead: "#e5484d",
} as const;

export const TRACKS = {
  video: { label: "V1", y: RULER_H + 8, h: 44 },
  audio: { label: "A1", y: RULER_H + 8 + 44 + 4, h: 36 },
} as const;

export function drawTimeline(ctx: CanvasRenderingContext2D, s: DrawState): void {
  ctx.fillStyle = COLORS.background;
  ctx.fillRect(0, 0, s.width, s.height);

  drawTrackBackgrounds(ctx, s);
  drawClips(ctx, s);
  drawRuler(ctx, s);
  drawGutter(ctx, s);
  drawPlayhead(ctx, s);
}

function drawTrackBackgrounds(ctx: CanvasRenderingContext2D, s: DrawState): void {
  ctx.fillStyle = COLORS.trackLine;
  for (const track of Object.values(TRACKS)) {
    ctx.fillRect(GUTTER, track.y, s.width - GUTTER, track.h);
  }
}

function drawClips(ctx: CanvasRenderingContext2D, s: DrawState): void {
  for (const [position, cut] of s.cuts.entries()) {
    const x0 = timeToX(cut.timeline_start, s.pxPerSec, s.scrollX);
    const x1 = timeToX(cut.timeline_start + cut.duration, s.pxPerSec, s.scrollX);
    if (x1 < GUTTER || x0 > s.width) continue;

    const isSelected = position === s.selected;
    const left = Math.max(x0, GUTTER);
    const w = Math.max(1, x1 - left - 1);
    const media = s.media[cut.asset];
    drawClip(ctx, TRACKS.video, left, w, isSelected ? COLORS.videoSelected : COLORS.video);
    const silent = media?.hasAudio === false;
    const soundColor = isSelected ? COLORS.audioSelected : COLORS.audio;
    drawClip(ctx, TRACKS.audio, left, w, silent ? COLORS.audioMuted : soundColor);
    if (silent && w > 70) mutedLabel(ctx, left, w);
    if (media?.thumbnails) drawThumbnails(ctx, cut, media.thumbnails, x0, left, left + w, s);
    if (media?.peaks && media.peaksPerSecond) {
      drawWaveform(ctx, cut, media.peaks, media.peaksPerSecond, x0, left, left + w, s, isSelected);
    }
    if (isSelected) {
      ctx.fillStyle = COLORS.selectedTint;
      ctx.fillRect(left, TRACKS.video.y, w, TRACKS.video.h);
    }
    if (isSelected) outline(ctx, left, w);
    if (w > 46) label(ctx, `${position + 1}  ${cut.duration.toFixed(1)}s`, left, w);
  }
}

/** Tiles are fixed-size and repeat along the clip, each showing the frame under its centre. */
function drawThumbnails(
  ctx: CanvasRenderingContext2D,
  cut: CutSummary,
  tiles: ThumbnailTiles,
  clipX0: number,
  left: number,
  right: number,
  s: DrawState,
): void {
  const { y, h } = TRACKS.video;
  ctx.save();
  ctx.beginPath();
  ctx.rect(left, y, right - left, h);
  ctx.clip();
  const skipped = Math.floor((left - clipX0) / tiles.tileWidth);
  for (let x = clipX0 + skipped * tiles.tileWidth; x < right; x += tiles.tileWidth) {
    const time = cut.start + (x + tiles.tileWidth / 2 - clipX0) / s.pxPerSec;
    const tile = clamp(Math.floor(time / tiles.step), 0, tiles.tiles - 1);
    ctx.drawImage(
      tiles.image,
      tile * tiles.tileWidth,
      0,
      tiles.tileWidth,
      tiles.tileHeight,
      x,
      y,
      tiles.tileWidth,
      h,
    );
  }
  ctx.restore();
}

/** One mirrored bar per screen pixel, as tall as the loudest peak under that pixel. */
function drawWaveform(
  ctx: CanvasRenderingContext2D,
  cut: CutSummary,
  peaks: Uint8Array,
  peaksPerSecond: number,
  clipX0: number,
  left: number,
  right: number,
  s: DrawState,
  selected: boolean,
): void {
  const { y, h } = TRACKS.audio;
  const middle = y + h / 2;
  ctx.fillStyle = selected ? COLORS.waveSelected : COLORS.wave;
  for (let x = Math.ceil(left); x < right; x += 1) {
    const from = cut.start + (x - clipX0) / s.pxPerSec;
    const first = Math.max(0, Math.floor(from * peaksPerSecond));
    const last = Math.min(peaks.length - 1, Math.floor((from + 1 / s.pxPerSec) * peaksPerSecond));
    let peak = 0;
    for (let i = first; i <= last; i += 1) peak = Math.max(peak, peaks[i] ?? 0);
    const half = Math.max(0.5, (peak / 255) * (h / 2 - 2));
    ctx.fillRect(x, middle - half, 1, half * 2);
  }
}

/** Says why the audio track of a clip is empty instead of leaving an unexplained dark block. */
function mutedLabel(ctx: CanvasRenderingContext2D, x: number, w: number): void {
  const { y, h } = TRACKS.audio;
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.fillStyle = COLORS.mutedText;
  ctx.font = "11px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  ctx.fillText("no audio", x + 8, y + h / 2);
  ctx.restore();
}

function drawClip(
  ctx: CanvasRenderingContext2D,
  track: { y: number; h: number },
  x: number,
  w: number,
  color: string,
): void {
  ctx.fillStyle = color;
  ctx.fillRect(x, track.y, w, track.h);
}

function outline(ctx: CanvasRenderingContext2D, x: number, w: number): void {
  const top = TRACKS.video.y;
  const bottom = TRACKS.audio.y + TRACKS.audio.h;
  ctx.strokeStyle = COLORS.selection;
  ctx.lineWidth = 1.5;
  ctx.strokeRect(x + 0.75, top + 0.75, w - 1.5, bottom - top - 1.5);
}

function label(ctx: CanvasRenderingContext2D, text: string, x: number, w: number): void {
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, TRACKS.video.y, w, TRACKS.video.h);
  ctx.clip();
  ctx.font = "11px system-ui, sans-serif";
  ctx.textBaseline = "top";
  ctx.fillStyle = COLORS.labelBackdrop;
  ctx.fillRect(x + 3, TRACKS.video.y + 3, ctx.measureText(text).width + 8, 15);
  ctx.fillStyle = COLORS.clipText;
  ctx.fillText(text, x + 7, TRACKS.video.y + 6);
  ctx.restore();
}

function drawRuler(ctx: CanvasRenderingContext2D, s: DrawState): void {
  ctx.fillStyle = COLORS.ruler;
  ctx.fillRect(GUTTER, 0, s.width - GUTTER, RULER_H);

  const step = rulerStep(s.pxPerSec);
  const decimals = step < 1 ? 1 : 0;
  const first = Math.floor(Math.max(0, (s.scrollX - GUTTER) / s.pxPerSec) / step);
  const last = Math.ceil((s.width - GUTTER + s.scrollX) / s.pxPerSec / step);

  ctx.font = "10px system-ui, sans-serif";
  ctx.textBaseline = "top";
  for (let i = first; i <= last; i += 1) {
    const time = i * step;
    const x = Math.round(timeToX(time, s.pxPerSec, s.scrollX)) + 0.5;
    if (x < GUTTER) continue;
    ctx.fillStyle = COLORS.rulerTick;
    ctx.fillRect(x, RULER_H - 8, 1, 8);
    ctx.fillStyle = COLORS.rulerText;
    ctx.fillText(formatRuler(time, decimals), x + 4, 4);
  }
}

function drawGutter(ctx: CanvasRenderingContext2D, s: DrawState): void {
  ctx.fillStyle = COLORS.gutter;
  ctx.fillRect(0, 0, GUTTER, s.height);
  ctx.fillStyle = COLORS.rulerText;
  ctx.font = "11px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  for (const track of Object.values(TRACKS)) {
    ctx.fillText(track.label, 14, track.y + track.h / 2);
  }
}

function drawPlayhead(ctx: CanvasRenderingContext2D, s: DrawState): void {
  const x = Math.round(timeToX(s.playhead, s.pxPerSec, s.scrollX)) + 0.5;
  if (x < GUTTER || x > s.width) return;
  ctx.fillStyle = COLORS.playhead;
  ctx.fillRect(x - 0.5, 0, 1.5, s.height);
  ctx.beginPath();
  ctx.moveTo(x - 6, 0);
  ctx.lineTo(x + 6, 0);
  ctx.lineTo(x, 10);
  ctx.closePath();
  ctx.fill();
}
