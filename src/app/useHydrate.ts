import { useEffect } from "react";
import { refreshProject } from "./refreshProject";

/** The backend keeps the project: a webview reload must show it again, not an empty editor. */
export function useHydrate(): void {
  useEffect(() => {
    void refreshProject();
  }, []);
}
