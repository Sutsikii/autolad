import { useTimelineStore } from "@/features/timeline/store";
import { api, call, type AssetSummary } from "@/ipc";
import { messageOf, notify } from "@/shared/notify";
import { Panel } from "@/shared/ui/Panel";
import { btn, btnPrimary, input } from "@/shared/ui/styles";
import { useAutomationStore } from "./store";

const FIELDS = [
  ["max_gap", "Bridge gaps under (s)"],
  ["margin", "Margin (s)"],
  ["min_segment", "Drop clips under (s)"],
] as const;

interface Props {
  /** Asset the silence removal runs on. */
  asset: AssetSummary | null;
}

export function AutomationPanel({ asset }: Props) {
  const assetId = asset?.id ?? null;
  const silent = asset !== null && !asset.has_audio;
  const settings = useAutomationStore((s) => s.settings);
  const noiseDb = useAutomationStore((s) => s.noiseDb);
  const running = useAutomationStore((s) => s.running);
  const update = useAutomationStore((s) => s.update);
  const setNoiseDb = useAutomationStore((s) => s.setNoiseDb);
  const reset = useAutomationStore((s) => s.reset);

  const run = async () => {
    if (!assetId) return;
    const { setRunning } = useAutomationStore.getState();
    setRunning(true);
    try {
      const before = useTimelineStore.getState().duration;
      const edl = await call(api.autoCut(assetId, settings, noiseDb));
      useTimelineStore.getState().setEdl(edl);
      useTimelineStore.getState().seek(0);
      const removed = Math.max(0, before - edl.total_duration);
      notify.info(
        `Silences removed: ${edl.cuts.length} clips, ${edl.total_duration.toFixed(1)} s` +
          (before > 0 ? ` (${removed.toFixed(1)} s shorter)` : ""),
      );
    } catch (error) {
      notify.error(messageOf(error));
    } finally {
      setRunning(false);
    }
  };

  return (
    <Panel title="Auto-cut" className="w-72 shrink-0">
      <div className="space-y-4 p-3">
        <div>
          <div className="mb-1 flex items-center justify-between text-xs text-neutral-300">
            <label htmlFor="noise">Silence threshold</label>
            <span className="font-mono text-neutral-500">{noiseDb} dB</span>
          </div>
          <input
            id="noise"
            type="range"
            min={-60}
            max={-10}
            step={1}
            value={noiseDb}
            onChange={(e) => setNoiseDb(Number(e.target.value))}
            className="w-full accent-sky-500"
          />
        </div>

        {FIELDS.map(([key, label]) => (
          <label key={key} className="flex items-center justify-between gap-3 text-xs text-neutral-300">
            {label}
            <input
              type="number"
              min={0}
              step={0.05}
              value={settings[key]}
              onChange={(e) => update({ [key]: Number(e.target.value) })}
              className={`${input} w-20 text-right`}
            />
          </label>
        ))}

        <div className="flex gap-2 pt-1">
          <button
            data-agent="auto-cut"
            className={`${btnPrimary} flex-1`}
            disabled={!assetId || silent || running}
            onClick={() => void run()}
          >
            {running ? "Analysing…" : "Remove silences"}
          </button>
          <button className={btn} onClick={reset} title="Restore default settings">
            Reset
          </button>
        </div>
        <p className="text-[11px] leading-relaxed text-neutral-500">
          {hint(asset)}
        </p>
      </div>
    </Panel>
  );
}

function hint(asset: AssetSummary | null): string {
  if (!asset) return "Select a clip in the Project panel first.";
  if (!asset.has_audio) {
    return "This clip has no audio track, so there is no silence to remove. Double-click it in the Project panel to add it to the timeline.";
  }
  return "Replaces the timeline with the speech parts of the selected clip.";
}
