import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useRef, useState } from "react";

/** Files dropped on the window arrive as real paths through Tauri, not as DOM File objects. */
export function useFileDrop(onPaths: (paths: string[]) => void): boolean {
  const [hovering, setHovering] = useState(false);
  const handler = useRef(onPaths);
  useEffect(() => {
    handler.current = onPaths;
  });

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    try {
      void getCurrentWebview()
        .onDragDropEvent(({ payload }) => {
          setHovering(payload.type === "enter" || payload.type === "over");
          if (payload.type === "drop") handler.current(payload.paths);
        })
        .then((stop) => {
          if (cancelled) stop();
          else unlisten = stop;
        });
    } catch {
      // Plain browser (vite preview, tests): there is no Tauri webview to listen to.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return hovering;
}
