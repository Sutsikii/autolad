import { useEffect, useRef } from "react";
import { cn } from "@/shared/lib/utils";
import { planMove, positionAt, type Point } from "./motion";
import { useAgentStore } from "./store";

const OFFSCREEN: Point = { x: -80, y: -80 };
const BUBBLE = "#d97757";
const FAILED = "#e5484d";

/** A second, virtual mouse pointer that shows what an AI agent is doing in the editor. */
export function AgentCursor() {
  const visible = useAgentStore((s) => s.visible);
  const label = useAgentStore((s) => s.label);
  const failed = useAgentStore((s) => s.failed);
  const moveSeq = useAgentStore((s) => s.moveSeq);
  const clickSeq = useAgentStore((s) => s.clickSeq);

  const cursorRef = useRef<HTMLDivElement>(null);
  const arrowRef = useRef<SVGSVGElement>(null);
  const rippleRef = useRef<HTMLDivElement>(null);
  const position = useRef<Point | null>(null);
  const frame = useRef(0);

  const place = (point: Point) => {
    position.current = point;
    const cursor = cursorRef.current;
    if (cursor) cursor.style.transform = `translate3d(${point.x}px, ${point.y}px, 0)`;
  };

  useEffect(() => {
    const target = useAgentStore.getState().target;
    if (!target) return;
    // First appearance: glide in from the bottom-right corner rather than popping up.
    const from = position.current ?? { x: window.innerWidth - 60, y: window.innerHeight - 60 };
    const plan = planMove(from, target, Math.random);
    const startedAt = performance.now();
    const step = (now: number) => {
      const elapsed = now - startedAt;
      place(positionAt(plan, elapsed));
      if (elapsed < plan.duration) frame.current = requestAnimationFrame(step);
    };
    frame.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame.current);
  }, [moveSeq]);

  useEffect(() => {
    if (clickSeq === 0) return;
    arrowRef.current?.animate(
      [{ transform: "scale(1)" }, { transform: "scale(0.8)" }, { transform: "scale(1)" }],
      { duration: 240, easing: "ease-out" },
    );
    rippleRef.current?.animate(
      [
        { transform: "scale(0.2)", opacity: 0.9 },
        { transform: "scale(1.6)", opacity: 0 },
      ],
      { duration: 520, easing: "ease-out" },
    );
  }, [clickSeq]);

  const color = failed ? FAILED : BUBBLE;
  return (
    <div
      ref={cursorRef}
      aria-hidden
      data-testid="agent-cursor"
      className={cn(
        "pointer-events-none fixed left-0 top-0 z-[100] transition-opacity duration-300",
        visible ? "opacity-100" : "opacity-0",
      )}
      style={{ transform: `translate3d(${OFFSCREEN.x}px, ${OFFSCREEN.y}px, 0)` }}
    >
      <div
        ref={rippleRef}
        className="absolute -left-4 -top-4 h-8 w-8 rounded-full border-2 opacity-0"
        style={{ borderColor: color }}
      />
      <svg
        ref={arrowRef}
        width="22"
        height="24"
        viewBox="0 0 22 24"
        className="absolute left-0 top-0 origin-top-left drop-shadow-md"
      >
        <path
          d="M1 1 L1 18 L5.6 13.8 L8.8 21.5 L11.6 20.3 L8.4 12.8 L14.6 12.8 Z"
          fill="#ffffff"
          stroke="#1a1a1a"
          strokeWidth="1.3"
          strokeLinejoin="round"
        />
      </svg>
      <div
        className="absolute left-4 top-5 max-w-64 truncate whitespace-nowrap rounded-full px-2.5 py-1 text-[11px] leading-none text-white shadow-lg"
        style={{ backgroundColor: color }}
      >
        <span className="font-semibold">Claude</span>
        {label && <span className="ml-1.5 opacity-90">{label}</span>}
      </div>
    </div>
  );
}
