import { useCallback, useEffect, useRef, useState } from "react";
import { drawTimeline } from "./draw";
import { clamp, cutIndexAt, maxScroll, RULER_H, xToTime } from "./layout";
import { useTimelineStore } from "./store";

interface Props {
  /** Bumped by the toolbar to ask for a "fit to window". */
  fitRequest: number;
}

/** The whole timeline is one <canvas>: no DOM node per clip. */
export function TimelineCanvas({ fitRequest }: Props) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scrubbing = useRef(false);
  const [size, setSize] = useState({ w: 0, h: 0 });

  const cuts = useTimelineStore((s) => s.cuts);
  const duration = useTimelineStore((s) => s.duration);
  const playhead = useTimelineStore((s) => s.playhead);
  const selected = useTimelineStore((s) => s.selected);
  const pxPerSec = useTimelineStore((s) => s.pxPerSec);
  const seek = useTimelineStore((s) => s.seek);
  const select = useTimelineStore((s) => s.select);
  const setZoom = useTimelineStore((s) => s.setZoom);
  const setScroll = useTimelineStore((s) => s.setScroll);
  const fit = useTimelineStore((s) => s.fit);
  const storedScroll = useTimelineStore((s) => s.scrollX);
  // Derived, so a shrinking edit or a wider window never leaves the view past the end.
  const scrollX = clamp(storedScroll, 0, maxScroll(duration, pxPerSec, size.w));

  useEffect(() => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setSize({ w: entry.contentRect.width, h: entry.contentRect.height });
    });
    observer.observe(wrap);
    return () => observer.disconnect();
  }, []);

  // Fit when the first clips arrive and whenever the toolbar asks for it.
  const hadContent = useRef(false);
  useEffect(() => {
    if (size.w === 0) return;
    const firstContent = duration > 0 && !hadContent.current;
    hadContent.current = duration > 0;
    if (firstContent) fit(size.w);
  }, [duration, size.w, fit]);

  useEffect(() => {
    if (fitRequest > 0 && size.w > 0) fit(size.w);
  }, [fitRequest, size.w, fit]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || size.w === 0) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(size.w * dpr);
    canvas.height = Math.round(size.h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    drawTimeline(ctx, {
      width: size.w,
      height: size.h,
      cuts,
      duration,
      pxPerSec,
      scrollX,
      playhead,
      selected,
    });
  }, [size, cuts, duration, pxPerSec, scrollX, playhead, selected]);

  // Native listener: React's wheel handler is passive and cannot stop the webview's own zoom.
  const onWheel = useCallback(
    (event: WheelEvent) => {
      event.preventDefault();
      const state = useTimelineStore.getState();
      if (event.ctrlKey) {
        setZoom(state.pxPerSec * (event.deltaY < 0 ? 1.15 : 1 / 1.15));
        return;
      }
      const delta = event.deltaX !== 0 ? event.deltaX : event.deltaY;
      const limit = maxScroll(state.duration, state.pxPerSec, size.w);
      setScroll(clamp(state.scrollX + delta, 0, limit));
    },
    [setZoom, setScroll, size.w],
  );
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, [onWheel]);

  const timeAt = (event: React.PointerEvent) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return xToTime(event.clientX - rect.left, pxPerSec, scrollX);
  };

  const onPointerDown = (event: React.PointerEvent<HTMLCanvasElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const time = timeAt(event);
    if (event.clientY - rect.top >= RULER_H) select(cutIndexAt(cuts, time));
    seek(time);
    scrubbing.current = true;
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLCanvasElement>) => {
    if (scrubbing.current) seek(timeAt(event));
  };

  const onPointerUp = () => {
    scrubbing.current = false;
  };

  return (
    <div ref={wrapRef} className="min-h-0 flex-1">
      <canvas
        ref={canvasRef}
        style={{ width: size.w, height: size.h }}
        className="block cursor-default"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
      />
    </div>
  );
}
