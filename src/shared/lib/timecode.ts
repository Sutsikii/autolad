export const TIMELINE_FPS = 30;

const pad = (n: number) => String(n).padStart(2, "0");

/** `HH:MM:SS:FF`, the notation editors read on the program monitor. */
export function formatTimecode(seconds: number, fps = TIMELINE_FPS): string {
  const total = Number.isFinite(seconds) ? Math.max(0, seconds) : 0;
  const whole = Math.floor(total);
  const frames = Math.min(fps - 1, Math.floor((total - whole) * fps));
  const h = Math.floor(whole / 3600);
  const m = Math.floor((whole % 3600) / 60);
  return `${pad(h)}:${pad(m)}:${pad(whole % 60)}:${pad(frames)}`;
}

/** Short form for rulers: `m:ss`, or `h:mm:ss` past an hour, with decimals for sub-second steps. */
export function formatRuler(seconds: number, decimals = 0): string {
  const total = Math.max(0, seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const sec = decimals > 0 ? s.toFixed(decimals).padStart(3 + decimals, "0") : pad(Math.floor(s));
  return h > 0 ? `${h}:${pad(m)}:${sec}` : `${m}:${sec}`;
}
