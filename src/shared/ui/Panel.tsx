import type { ReactNode } from "react";
import { cn } from "@/shared/lib/utils";

interface Props {
  title: string;
  actions?: ReactNode;
  className?: string;
  /** Marks the panel so the agent cursor can find it on screen. */
  agent?: string;
  children: ReactNode;
}

/** Docked panel with a thin title bar, as in a video editor workspace. */
export function Panel({ title, actions, className, agent, children }: Props) {
  return (
    <section
      data-agent={agent}
      className={cn("flex min-h-0 min-w-0 flex-col bg-[#232323]", className)}
    >
      <header className="flex h-8 shrink-0 items-center justify-between gap-2 border-b border-black/60 px-3">
        <h2 className="truncate text-[11px] font-medium uppercase tracking-wide text-neutral-400">
          {title}
        </h2>
        {actions}
      </header>
      <div className="flex min-h-0 flex-1 flex-col">{children}</div>
    </section>
  );
}
