import { useEffect, useState } from "react";
import { api } from "@/ipc";

/** Reports whether the Rust side answers over IPC. */
export function usePing() {
  const [status, setStatus] = useState("connecting…");
  useEffect(() => {
    api.ping().then(
      (r) => setStatus(r),
      () => setStatus("unreachable"),
    );
  }, []);
  return status;
}
