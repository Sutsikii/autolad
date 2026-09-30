// Single entry point to the backend: the rest of the front never imports bindings.ts directly.
export { commands as api } from "./bindings";
export type { AppError, Asset, Edl, SilenceSettings, TimeRange } from "./bindings";
