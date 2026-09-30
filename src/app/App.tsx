import { AutomationPanel } from "@/features/automation/AutomationPanel";
import { usePing } from "@/features/automation/usePing";

export function App() {
  const status = usePing();
  return (
    <main className="min-h-screen bg-neutral-950 p-8 text-neutral-100">
      <h1 className="text-2xl font-semibold">AutoLad</h1>
      <p className="mt-1 text-sm text-neutral-400">Backend: {status}</p>
      <AutomationPanel />
    </main>
  );
}
