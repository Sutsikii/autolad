import { fileName } from "@/shared/lib/path";
import { btn } from "@/shared/ui/styles";
import { newProject, openProject, saveProject, saveProjectAs } from "./actions";
import { useProjectStore } from "./store";

/** Project name plus New / Open / Save / Save as, in the top bar. */
export function ProjectMenu() {
  const file = useProjectStore((s) => s.file);
  return (
    <div className="flex items-center gap-3">
      <span className="flex items-baseline gap-2">
        <span className="text-sm font-semibold tracking-wide text-neutral-100">AutoLad</span>
        <span
          data-agent="project-title"
          title={file ?? "Not saved yet"}
          className="max-w-64 truncate text-xs text-neutral-500"
        >
          {file ? fileName(file) : "Untitled sequence"}
        </span>
      </span>
      <div className="flex items-center gap-1.5">
        <button className={btn} onClick={() => void newProject()} title="New project (Ctrl+N)">
          New
        </button>
        <button className={btn} onClick={() => void openProject()} title="Open project (Ctrl+O)">
          Open
        </button>
        <button className={btn} onClick={() => void saveProject()} title="Save (Ctrl+S)">
          Save
        </button>
        <button
          className={btn}
          onClick={() => void saveProjectAs()}
          title="Save as… (Ctrl+Shift+S)"
        >
          Save as…
        </button>
      </div>
    </div>
  );
}
