import { useState } from "react";
import { addAssetToTimeline } from "@/features/timeline/actions";
import type { AssetSummary } from "@/ipc";
import { cn } from "@/shared/lib/utils";
import { fileName } from "@/shared/lib/path";
import { formatTimecode } from "@/shared/lib/timecode";
import { Panel } from "@/shared/ui/Panel";
import { btn, input } from "@/shared/ui/styles";
import { useLibraryStore } from "./store";
import { useFileDrop } from "./useFileDrop";

function describe(asset: AssetSummary): string {
  const size = asset.width && asset.height ? `${asset.width}×${asset.height}` : "";
  const fps = asset.fps ? `${Math.round(asset.fps)} fps` : "";
  return [formatTimecode(asset.duration), size, fps].filter(Boolean).join(" · ");
}

export function LibraryPanel() {
  const assets = useLibraryStore((s) => s.assets);
  const selectedId = useLibraryStore((s) => s.selectedId);
  const importing = useLibraryStore((s) => s.importing);
  const select = useLibraryStore((s) => s.select);
  const importPath = useLibraryStore((s) => s.importPath);
  const [pathInput, setPathInput] = useState("");

  const importAll = async (paths: string[]) => {
    for (const path of paths) await importPath(path);
  };
  const hovering = useFileDrop((paths) => void importAll(paths));

  const submit = () => {
    void importPath(pathInput);
    setPathInput("");
  };

  return (
    <Panel title="Project" className="w-72 shrink-0">
      <div data-agent="import" className="flex gap-1.5 border-b border-black/40 p-2">
        <input
          value={pathInput}
          onChange={(e) => setPathInput(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && submit()}
          placeholder="Paste a file path…"
          className={cn(input, "min-w-0 flex-1")}
        />
        <button className={btn} disabled={!pathInput.trim()} onClick={submit}>
          Import
        </button>
      </div>

      <ul
        className={cn(
          "min-h-0 flex-1 overflow-y-auto p-2",
          hovering && "outline outline-2 -outline-offset-4 outline-sky-500",
        )}
      >
        {assets.map((asset) => (
          <li key={asset.id}>
            <button
              onClick={() => select(asset.id)}
              onDoubleClick={() => void addAssetToTimeline(asset)}
              title="Double-click to add to the timeline"
              className={cn(
                "w-full rounded px-2 py-1.5 text-left",
                asset.id === selectedId ? "bg-sky-700/40" : "hover:bg-neutral-700/40",
              )}
            >
              <div className="truncate text-xs text-neutral-100">{fileName(asset.path)}</div>
              <div className="text-[11px] text-neutral-500">{describe(asset)}</div>
            </button>
          </li>
        ))}
        {assets.length === 0 && (
          <li className="mt-6 px-4 text-center text-xs leading-relaxed text-neutral-500">
            Drop video files here
            <br />
            or paste a path above.
          </li>
        )}
        {importing > 0 && (
          <li className="px-2 py-1.5 text-xs text-sky-400">Importing… ({importing})</li>
        )}
      </ul>
    </Panel>
  );
}
