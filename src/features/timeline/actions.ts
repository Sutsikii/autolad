import { api, call, type AssetSummary, type EdlOp } from "@/ipc";
import { messageOf, notify } from "@/shared/notify";
import { cutIndexAt } from "./layout";
import { useTimelineStore } from "./store";

/** Sends edit operations to the backend and shows the resulting EDL. */
async function applyOps(ops: EdlOp[]): Promise<boolean> {
  try {
    const edl = await call(api.editEdl(ops));
    useTimelineStore.getState().setEdl(edl);
    return true;
  } catch (error) {
    notify.error(messageOf(error));
    return false;
  }
}

/** Undo / redo the last timeline change, whoever made it (the user or an agent). */
export async function stepHistory(direction: "undo" | "redo"): Promise<void> {
  try {
    const step = await call(direction === "undo" ? api.undo() : api.redo());
    useTimelineStore.getState().setEdl(step.edl);
    notify.info(`${direction === "undo" ? "Undid" : "Redid"}: ${step.change}`);
  } catch (error) {
    notify.error(messageOf(error));
  }
}

export async function addAssetToTimeline(asset: AssetSummary): Promise<void> {
  const { cuts } = useTimelineStore.getState();
  const added = await applyOps([
    { op: "insert", index: cuts.length, asset: asset.id, start: 0, end: asset.duration },
  ]);
  if (added) notify.info("Clip added to the timeline");
}

export async function splitAtPlayhead(): Promise<void> {
  const { cuts, playhead } = useTimelineStore.getState();
  const index = cutIndexAt(cuts, playhead);
  const cut = index === null ? undefined : cuts[index];
  if (index === null || !cut) {
    notify.error("Move the playhead over a clip to split it");
    return;
  }
  const sourceTime = cut.start + (playhead - cut.timeline_start);
  await applyOps([{ op: "split", index, at: sourceTime }]);
}

/** Ripple delete: the following clips close the gap, as the EDL is contiguous. */
export async function deleteSelected(): Promise<void> {
  const { selected } = useTimelineStore.getState();
  if (selected === null) {
    notify.error("Select a clip first");
    return;
  }
  if (await applyOps([{ op: "delete", index: selected }])) {
    useTimelineStore.getState().select(null);
  }
}

export async function moveSelected(offset: -1 | 1): Promise<void> {
  const { selected, cuts } = useTimelineStore.getState();
  if (selected === null) return;
  const to = selected + offset;
  if (to < 0 || to >= cuts.length) return;
  if (await applyOps([{ op: "move", from: selected, to }])) {
    useTimelineStore.getState().select(to);
  }
}
