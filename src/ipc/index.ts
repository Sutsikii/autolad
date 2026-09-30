// Single entry point to the backend: the rest of the front never imports bindings.ts directly.
import { commands, events, type AppError, type Result } from "./bindings";

export { commands as api, events };
export type { Result };
export type {
  AgentActivity,
  AppError,
  Asset,
  AssetSummary,
  CutSummary,
  Edl,
  EdlOp,
  EdlSummary,
  HistoryStatus,
  JobStatus,
  ProjectStatus,
  SilenceSettings,
  TextCut,
  TextEditReport,
  TimeRange,
  TranscriptEntry,
  TranscriptReport,
} from "./bindings";

/** Unwraps a command result: the error message becomes a thrown `Error`. */
export async function call<T>(pending: Promise<Result<T, AppError>>): Promise<T> {
  const result = await pending;
  if (result.status === "ok") return result.data;
  throw new Error(result.error.message);
}
