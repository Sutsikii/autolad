export interface Point {
  x: number;
  y: number;
}

export interface MovePlan {
  from: Point;
  control: Point;
  to: Point;
  duration: number;
  /** Phase of the hand tremor, so two moves never wobble identically. */
  wobble: number;
}

const MIN_TRAVEL_MS = 420;
// Stays under the pause the backend leaves before acting, so the cursor has arrived by then.
const MAX_TRAVEL_MS = 820;
const TREMOR_PX = 1.4;

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

export const distance = (a: Point, b: Point) => Math.hypot(b.x - a.x, b.y - a.y);

/** Slow start, fast middle, slow arrival: how a hand moves a mouse. */
export function easeInOutCubic(t: number): number {
  const x = clamp(t, 0, 1);
  return x < 0.5 ? 4 * x * x * x : 1 - (-2 * x + 2) ** 3 / 2;
}

/** Far targets take longer, within bounds. */
export function travelDuration(px: number): number {
  return clamp(380 + px * 0.55, MIN_TRAVEL_MS, MAX_TRAVEL_MS);
}

/**
 * Humans do not move in straight lines: the path bows to one side by 8-20 % of its length.
 * `random` returns values in [0, 1), injected so tests are deterministic.
 */
export function planMove(from: Point, to: Point, random: () => number): MovePlan {
  const length = distance(from, to);
  const side = random() < 0.5 ? -1 : 1;
  const bend = side * (0.08 + random() * 0.12) * length;
  const mid = { x: (from.x + to.x) / 2, y: (from.y + to.y) / 2 };
  // Unit normal of the segment; a zero-length move has nothing to bow.
  const normal = length === 0 ? { x: 0, y: 0 } : { x: -(to.y - from.y) / length, y: (to.x - from.x) / length };
  return {
    from,
    to,
    control: { x: mid.x + normal.x * bend, y: mid.y + normal.y * bend },
    duration: travelDuration(length),
    wobble: random() * Math.PI * 2,
  };
}

/** Cursor position `elapsed` ms into the move. */
export function positionAt(plan: MovePlan, elapsed: number): Point {
  const progress = clamp(elapsed / plan.duration, 0, 1);
  const t = easeInOutCubic(progress);
  const u = 1 - t;
  const x = u * u * plan.from.x + 2 * u * t * plan.control.x + t * t * plan.to.x;
  const y = u * u * plan.from.y + 2 * u * t * plan.control.y + t * t * plan.to.y;
  // The tremor is strongest mid-move and dies out, so the cursor lands exactly on target.
  const strength = Math.sin(progress * Math.PI) * TREMOR_PX;
  return {
    x: x + Math.sin(progress * 23 + plan.wobble) * strength,
    y: y + Math.cos(progress * 19 + plan.wobble) * strength,
  };
}
