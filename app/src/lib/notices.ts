// Which engine notices the recording view shows, and how.

import type { StringKey } from "./strings";
import type { Notice, Track } from "./types";

export interface Banner {
  key: string;
  level: "info" | "warn" | "error";
  text: StringKey;
  params?: Record<string, string>;
  /// Offer a button that opens the privacy pane for this permission.
  settings?: "mic" | "sys";
}

/// Banners for the current notices. `modelInstalled` false shows one "no model" banner
/// instead of load errors.
export function banners(notices: Notice[], modelInstalled: boolean): Banner[] {
  const out: Banner[] = [];
  if (!modelInstalled) out.push({ key: "model", level: "warn", text: "notice.modelMissing" });
  for (const notice of notices) {
    switch (notice.kind) {
      case "modelLoading":
        if (modelInstalled) out.push({ key: "model", level: "info", text: "notice.modelLoading" });
        break;
      case "modelFailed":
        if (modelInstalled) {
          out.push({ key: "model", level: "error", text: "notice.modelFailed", params: { error: notice.error } });
        }
        break;
      case "sourceFailed":
        out.push({
          key: `source-${notice.track}`,
          level: notice.track === "mic" ? "error" : "warn",
          text: notice.track === "mic" ? "notice.micFailed" : "notice.sysFailed",
          params: { error: notice.error },
        });
        break;
      case "permissionDenied":
        out.push({
          key: `source-${notice.track}`,
          level: notice.track === "mic" ? "error" : "warn",
          text: notice.track === "mic" ? "notice.micDenied" : "notice.sysDenied",
          settings: notice.track,
        });
        break;
      case "bleed":
        out.push({ key: "bleed", level: "info", text: "notice.bleed" });
        break;
      case "storeFailed":
        out.push({ key: "store", level: "error", text: "notice.storeFailed", params: { error: notice.error } });
        break;
      case "writeFailed":
        out.push({
          key: `write-${notice.track}`,
          level: "error",
          text: "notice.writeFailed",
          params: { error: notice.error },
        });
        break;
      // Loaded, started and device changes show in the meters; auto-stop as a toast.
      default:
        break;
    }
  }
  return out;
}

/// The device currently recording `track`, from the latest "started" notice.
export function deviceName(notices: Notice[], track: Track): string | null {
  for (let i = notices.length - 1; i >= 0; i--) {
    const notice = notices[i]!;
    if (notice.kind === "sourceStarted" && notice.track === track) return notice.name;
    if ("track" in notice && notice.track === track) return null;
  }
  return null;
}
