import { describe, expect, it } from "vitest";
import { distance, easeInOutCubic, planMove, positionAt, travelDuration } from "./motion";

const fixed = (...values: number[]) => {
  let i = 0;
  return () => values[i++ % values.length] ?? 0;
};

describe("easeInOutCubic", () => {
  it("starts slow, ends slow and is symmetric", () => {
    expect(easeInOutCubic(0)).toBe(0);
    expect(easeInOutCubic(1)).toBe(1);
    expect(easeInOutCubic(0.5)).toBeCloseTo(0.5);
    expect(easeInOutCubic(0.1)).toBeLessThan(0.1);
    expect(easeInOutCubic(0.9)).toBeGreaterThan(0.9);
    expect(easeInOutCubic(-3)).toBe(0);
    expect(easeInOutCubic(7)).toBe(1);
  });
});

describe("travelDuration", () => {
  it("grows with distance but stays within bounds", () => {
    expect(travelDuration(0)).toBe(420);
    expect(travelDuration(400)).toBeGreaterThan(travelDuration(100));
    expect(travelDuration(99999)).toBe(820);
  });
});

describe("planMove / positionAt", () => {
  const from = { x: 0, y: 0 };
  const to = { x: 600, y: 0 };

  it("starts on the origin and lands exactly on the target", () => {
    const plan = planMove(from, to, fixed(0.2, 0.5, 0.3));
    expect(positionAt(plan, 0)).toEqual(from);
    const end = positionAt(plan, plan.duration);
    expect(end.x).toBeCloseTo(to.x);
    expect(end.y).toBeCloseTo(to.y);
    expect(positionAt(plan, plan.duration * 5)).toEqual(end);
  });

  it("does not travel in a straight line", () => {
    const plan = planMove(from, to, fixed(0.9, 1, 0));
    const middle = positionAt(plan, plan.duration / 2);
    expect(Math.abs(middle.y)).toBeGreaterThan(20);
  });

  it("bows to either side depending on the dice", () => {
    const left = positionAt(planMove(from, to, fixed(0.1, 0.5, 0)), 400);
    const right = positionAt(planMove(from, to, fixed(0.9, 0.5, 0)), 400);
    expect(Math.sign(left.y)).toBe(-Math.sign(right.y));
  });

  it("covers the distance monotonically along the main axis", () => {
    const plan = planMove(from, to, fixed(0.3, 0.4, 0.5));
    let last = -Infinity;
    for (let ms = 0; ms <= plan.duration; ms += 20) {
      const { x } = positionAt(plan, ms);
      expect(x).toBeGreaterThanOrEqual(last - 3);
      last = x;
    }
  });

  it("handles a zero-length move", () => {
    const plan = planMove(from, from, fixed(0.5));
    expect(positionAt(plan, plan.duration / 2)).toBeDefined();
    expect(distance(positionAt(plan, plan.duration), from)).toBeLessThan(0.001);
  });
});
