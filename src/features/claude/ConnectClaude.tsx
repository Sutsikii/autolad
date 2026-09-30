import { useEffect, useRef, useState } from "react";
import type { AppConnection, ClaudeApp } from "@/ipc";
import { cn } from "@/shared/lib/utils";
import { messageOf, notify } from "@/shared/notify";
import { btn, btnPrimary } from "@/shared/ui/styles";
import { useClaudeStore } from "./store";

const APPS: [ClaudeApp, string, string][] = [
  ["desktop", "Claude Desktop", "Restart Claude Desktop after connecting."],
  ["code", "Claude Code", "New sessions see AutoLad; type /mcp in a running one."],
];

/** Registers AutoLad as an MCP server in the Claude apps, so Claude can edit with it. */
export function ConnectClaude() {
  const [open, setOpen] = useState(false);
  const setup = useClaudeStore((s) => s.setup);
  const load = useClaudeStore((s) => s.load);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent | KeyboardEvent) => {
      const outside = event instanceof MouseEvent && !root.current?.contains(event.target as Node);
      if (outside || (event instanceof KeyboardEvent && event.key === "Escape")) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", close);
    };
  }, [open]);

  const connected = setup !== null && (setup.desktop.connected || setup.code.connected);
  return (
    <div ref={root} className="relative">
      <button
        className={cn(btn, "flex items-center gap-1.5")}
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        title="Let Claude edit with AutoLad"
      >
        <span className={cn("h-1.5 w-1.5 rounded-full", connected ? "bg-emerald-400" : "bg-neutral-500")} />
        Connect Claude
      </button>
      {open && (
        <div className="absolute right-0 top-8 z-20 w-80 space-y-3 rounded border border-black bg-[#2a2a2a] p-3 shadow-xl">
          {setup === null ? (
            <p className="text-xs text-neutral-500">Looking for Claude…</p>
          ) : (
            <>
              {APPS.map(([app, name, hint]) => (
                <AppRow key={app} app={app} name={name} hint={hint} state={setup[app]} />
              ))}
              {!setup.code.installed && <ManualCommand command={setup.code_command} />}
              <p className="border-t border-black/40 pt-2 text-[11px] leading-relaxed text-neutral-500">
                Open AutoLad before Claude connects: you will see it work here, live.
              </p>
            </>
          )}
        </div>
      )}
    </div>
  );
}

interface RowProps {
  app: ClaudeApp;
  name: string;
  hint: string;
  state: AppConnection;
}

function AppRow({ app, name, hint, state }: RowProps) {
  const connecting = useClaudeStore((s) => s.connecting);
  const connect = useClaudeStore((s) => s.connect);
  const status = state.connected ? "Connected" : state.installed ? "Not connected" : "Not found";
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="min-w-0">
        <p className="text-xs text-neutral-100">{name}</p>
        <p className={cn("text-[11px]", state.connected ? "text-emerald-400" : "text-neutral-500")}>
          {status}
        </p>
        {state.installed && <p className="text-[11px] leading-snug text-neutral-500">{hint}</p>}
      </div>
      <button
        className={state.connected ? btn : btnPrimary}
        disabled={!state.installed || connecting !== null}
        onClick={() => void connect(app)}
      >
        {connecting === app ? "Connecting…" : state.connected ? "Reconnect" : "Connect"}
      </button>
    </div>
  );
}

/** Without the claude command, the user can still register AutoLad from a terminal. */
function ManualCommand({ command }: { command: string }) {
  const copy = () => {
    navigator.clipboard.writeText(command).then(
      () => notify.info("Command copied"),
      (error: unknown) => notify.error(messageOf(error)),
    );
  };
  return (
    <div className="space-y-1">
      <p className="text-[11px] text-neutral-500">Or run this in a terminal:</p>
      <div className="flex gap-2">
        <code className="min-w-0 flex-1 select-text truncate rounded bg-neutral-900 px-2 py-1 text-[11px] text-neutral-300">
          {command}
        </code>
        <button className={btn} onClick={copy}>
          Copy
        </button>
      </div>
    </div>
  );
}
