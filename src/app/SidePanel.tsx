import { AutomationPanel } from "@/features/automation/AutomationPanel";
import { TranscriptPanel } from "@/features/transcript/TranscriptPanel";
import type { AssetSummary } from "@/ipc";
import { cn } from "@/shared/lib/utils";
import { useSidebarStore, type SidebarTab } from "./sidebarStore";

const TABS: [SidebarTab, string][] = [
  ["auto-cut", "Auto-cut"],
  ["transcript", "Transcript"],
];

/** Right-hand dock: one tab per tool that works on the selected clip. */
export function SidePanel({ asset }: { asset: AssetSummary | null }) {
  const tab = useSidebarStore((s) => s.tab);
  const setTab = useSidebarStore((s) => s.setTab);
  return (
    <section className="flex min-h-0 w-72 min-w-0 shrink-0 flex-col bg-[#232323]">
      <header className="flex h-8 shrink-0 items-stretch border-b border-black/60" role="tablist">
        {TABS.map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={tab === id}
            data-agent={id === "transcript" ? "transcript" : undefined}
            onClick={() => setTab(id)}
            className={cn(
              "px-3 text-[11px] font-medium uppercase tracking-wide transition-colors",
              tab === id
                ? "border-b-2 border-sky-500 text-neutral-100"
                : "text-neutral-500 hover:text-neutral-300",
            )}
          >
            {label}
          </button>
        ))}
      </header>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        {tab === "auto-cut" ? <AutomationPanel asset={asset} /> : <TranscriptPanel asset={asset} />}
      </div>
    </section>
  );
}
