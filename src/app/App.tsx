import { AgentCursor } from "@/features/agent/AgentCursor";
import { useAgentEvents } from "@/features/agent/useAgentEvents";
import { usePing } from "@/features/automation/usePing";
import { ExportControls } from "@/features/export/ExportControls";
import { LibraryPanel } from "@/features/library/LibraryPanel";
import { useLibraryStore } from "@/features/library/store";
import { ProjectMenu } from "@/features/project/ProjectMenu";
import { PreviewPanel } from "@/features/preview/PreviewPanel";
import { TimelinePanel } from "@/features/timeline/TimelinePanel";
import { useNotice } from "@/shared/notify";
import { cn } from "@/shared/lib/utils";
import { SidePanel } from "./SidePanel";
import { useHydrate } from "./useHydrate";
import { useShortcuts } from "./useShortcuts";

function TopBar() {
  return (
    <header className="flex h-11 shrink-0 items-center justify-between border-b border-black bg-[#1a1a1a] px-4">
      <ProjectMenu />
      <ExportControls />
    </header>
  );
}

function StatusBar() {
  const backend = usePing();
  const notice = useNotice((s) => s.notice);
  return (
    <footer className="flex h-6 shrink-0 items-center justify-between border-t border-black bg-[#1a1a1a] px-3 text-[11px]">
      <span
        className={cn("truncate", notice?.kind === "error" ? "text-red-400" : "text-neutral-400")}
        role="status"
      >
        {notice?.text ?? "Ready"}
      </span>
      <span className="shrink-0 pl-4 text-neutral-600">Backend: {backend}</span>
    </footer>
  );
}

export function App() {
  useHydrate();
  useShortcuts();
  useAgentEvents();
  const selectedAsset = useLibraryStore(
    (s) => s.assets.find((a) => a.id === s.selectedId) ?? null,
  );

  return (
    <div className="flex h-screen select-none flex-col bg-black text-neutral-200">
      <TopBar />
      <div className="flex min-h-0 flex-1 gap-px">
        <LibraryPanel />
        <PreviewPanel />
        <SidePanel asset={selectedAsset} />
      </div>
      <div className="h-px shrink-0" />
      <TimelinePanel />
      <StatusBar />
      <AgentCursor />
    </div>
  );
}
