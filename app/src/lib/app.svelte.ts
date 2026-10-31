// App-wide state: info, settings, the meeting list, the live recording and navigation.

import { asCommandError, api, events } from "./ipc";
import { applyDelta, fromSnapshot, isActive, type LiveView } from "./live";
import { dateTime } from "./format";
import { errorText, locale, t } from "./i18n.svelte";
import type { AppInfo, CommandError, Delta, Meeting, Settings } from "./types";

export type View =
  | { name: "home" }
  | { name: "live" }
  | { name: "meeting"; id: number; focus: number | null }
  | { name: "settings" };

class AppStore {
  info = $state<AppInfo | null>(null);
  settings = $state<Settings | null>(null);
  meetings = $state.raw<Meeting[]>([]);
  /// Replaced on every delta (not deeply reactive: transcripts get long).
  live = $state.raw<LiveView | null>(null);
  view = $state<View>({ name: "home" });
  /// Bumped when the library changes, so open views reload.
  libraryVersion = $state(0);
  starting = $state(false);
  toast = $state<string | null>(null);
  /// Deltas that arrive before the first snapshot.
  private early: Delta[] | null = [];

  async init(): Promise<void> {
    // Listen first, so nothing between the snapshot and the subscription is lost.
    await events.live((delta) => this.onDelta(delta));
    await events.libraryChanged(() => {
      this.libraryVersion++;
      void this.loadMeetings();
    });
    await events.appError((error) => this.showError(error));
    const [info, settings] = await Promise.all([api.appInfo(), api.getSettings()]);
    this.info = info;
    this.settings = settings;
    locale.value = settings.language;
    await this.loadMeetings();
    const snapshot = await api.liveSnapshot().catch(() => null);
    let live = snapshot ? fromSnapshot(snapshot, performance.now()) : null;
    for (const delta of this.early ?? []) live = applyDelta(live, delta, performance.now());
    this.early = null;
    this.live = live;
    if (isActive(live)) this.view = { name: "live" };
    if (info.pendingError) this.showError(info.pendingError);
  }

  async loadMeetings(): Promise<void> {
    try {
      this.meetings = await api.listMeetings();
    } catch (error) {
      this.showError(asCommandError(error));
    }
  }

  private onDelta(delta: Delta): void {
    if (this.early) {
      this.early.push(delta);
      return;
    }
    const started = !this.live || this.live.meeting !== delta.meeting;
    const next = applyDelta(this.live, delta, performance.now());
    // A new object each time so views that read `live` update.
    this.live = { ...next };
    if (started) this.view = { name: "live" };
    if (delta.finished) {
      const autoStopped = next.notices.some((notice) => notice.kind === "autoStopped");
      if (autoStopped) this.toast = t("notice.autoStopped");
      if (this.view.name === "live") this.view = { name: "meeting", id: delta.meeting, focus: null };
    }
  }

  get recording(): boolean {
    return isActive(this.live);
  }

  async startRecording(): Promise<void> {
    if (this.starting || this.recording) {
      this.view = { name: "live" };
      return;
    }
    this.starting = true;
    try {
      await api.startRecording();
      this.view = { name: "live" };
    } catch (error) {
      this.showError(asCommandError(error));
    } finally {
      this.starting = false;
    }
  }

  showError(error: CommandError): void {
    this.toast = errorText(error);
  }

  /// A meeting's title, or "<date> 的会议" when it has none.
  title(meeting: Pick<Meeting, "title" | "startedAtMs">): string {
    return meeting.title.trim() || t("meeting.untitled", { date: dateTime(meeting.startedAtMs, locale.value) });
  }
}

export const app = new AppStore();
