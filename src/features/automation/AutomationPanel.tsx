import { useAutomationStore } from "./store";

const FIELDS = [
  ["max_gap", "Max gap to bridge (s)"],
  ["margin", "Margin (s)"],
  ["min_segment", "Min segment (s)"],
] as const;

export function AutomationPanel() {
  const settings = useAutomationStore((s) => s.settings);
  const update = useAutomationStore((s) => s.update);

  return (
    <section className="mt-6 max-w-sm space-y-3">
      <h2 className="text-lg font-medium">Silence removal</h2>
      {FIELDS.map(([key, label]) => (
        <label key={key} className="flex items-center justify-between gap-4 text-sm">
          {label}
          <input
            type="number"
            min={0}
            step={0.05}
            value={settings[key]}
            onChange={(e) => update({ [key]: Number(e.target.value) })}
            className="w-24 rounded bg-neutral-800 px-2 py-1"
          />
        </label>
      ))}
    </section>
  );
}
