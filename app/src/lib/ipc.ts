// Typed wrappers for the Rust commands and events. Argument names are camelCase here and
// snake_case in Rust; Tauri converts them.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AppInfo,
  CommandError,
  Delta,
  Device,
  MeetingDetail,
  Meeting,
  Permissions,
  SearchHit,
  Settings,
  Snapshot,
} from "./types";

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  listMeetings: () => invoke<Meeting[]>("list_meetings"),
  search: (query: string, meetingId: number | null = null) =>
    invoke<SearchHit[]>("search", { query, meetingId }),
  getMeeting: (id: number) => invoke<MeetingDetail>("get_meeting", { id }),
  renameMeeting: (id: number, title: string) => invoke<void>("rename_meeting", { id, title }),
  startRecording: () => invoke<number>("start_recording"),
  stopRecording: () => invoke<void>("stop_recording"),
  pauseRecording: () => invoke<void>("pause_recording"),
  resumeRecording: () => invoke<void>("resume_recording"),
  liveSnapshot: () => invoke<Snapshot | null>("live_snapshot"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  listInputDevices: () => invoke<Device[]>("list_input_devices"),
  permissions: () => invoke<Permissions>("permissions"),
  openPrivacySettings: (kind: "mic" | "sys") => invoke<void>("open_privacy_settings", { kind }),
  smokeReport: (report: unknown) => invoke<void>("smoke_report", { report }),
};

export const events = {
  live: (handler: (delta: Delta) => void): Promise<UnlistenFn> =>
    listen<Delta>("live", (event) => handler(event.payload)),
  libraryChanged: (handler: () => void): Promise<UnlistenFn> =>
    listen("library-changed", () => handler()),
  appError: (handler: (error: CommandError) => void): Promise<UnlistenFn> =>
    listen<CommandError>("app-error", (event) => handler(event.payload)),
};

/// Commands reject with a CommandError; anything else becomes one.
export function asCommandError(error: unknown): CommandError {
  if (error && typeof error === "object" && "code" in error && "message" in error) {
    return error as CommandError;
  }
  return { code: "unknown", message: String(error) };
}
