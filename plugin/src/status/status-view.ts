import { ItemView, WorkspaceLeaf, setIcon } from "obsidian";
import type EchoNotePlugin from "../main";
import type { EchoNoteStatus } from "./status-types";

export const ECHONOTE_STATUS_VIEW_TYPE = "echonote-status-view";

type StatusRow = {
  label: string;
  value: string;
  tone?: string;
  variant?: "badge" | "code";
};

type StatusSection = {
  title: string;
  rows: StatusRow[];
};

type StatusSummaryItem = {
  label: string;
  value: string;
  tone: string;
};

export class EchoNoteStatusView extends ItemView {
  private unsubscribe: (() => void) | null = null;

  constructor(
    leaf: WorkspaceLeaf,
    private readonly plugin: EchoNotePlugin
  ) {
    super(leaf);
  }

  getViewType(): string {
    return ECHONOTE_STATUS_VIEW_TYPE;
  }

  getDisplayText(): string {
    return "EchoNote Status";
  }

  getIcon(): string {
    return "mic";
  }

  async onOpen(): Promise<void> {
    this.unsubscribe = this.plugin.statusStore.subscribe((status) => {
      this.render(status);
    });
  }

  async onClose(): Promise<void> {
    this.unsubscribe?.();
    this.unsubscribe = null;
  }

  private render(status: EchoNoteStatus): void {
    const container = this.contentEl;
    container.empty();
    container.addClass("echonote-status-view");

    const header = container.createDiv({ cls: "echonote-status-header" });
    const title = header.createDiv({ cls: "echonote-status-title" });
    title.createEl("h2", { text: "EchoNote" });
    title.createDiv({ cls: "echonote-status-kicker", text: "ASR Console" });
    this.addStatusPill(header, status.companionStatus);

    const summary: StatusSummaryItem[] = [
      { label: "Service", value: status.asrService, tone: this.statusTone(status.asrService) },
      { label: "Model", value: status.model, tone: this.statusTone(status.model) },
      { label: "Recording", value: status.recording, tone: this.statusTone(status.recording) }
    ];
    const summaryEl = container.createDiv({ cls: "echonote-status-summary" });
    for (const item of summary) {
      this.addSummaryItem(summaryEl, item);
    }

    const sections: StatusSection[] = [
      {
        title: "Runtime",
        rows: [
          { label: "ASR Runtime", value: this.formatAsrRuntime(status) },
          { label: "ASR Service", value: status.asrService, tone: this.statusTone(status.asrService), variant: "badge" },
          { label: "Model", value: status.model, tone: this.statusTone(status.model), variant: "badge" },
          { label: "Selected Model", value: status.selectedModel, variant: "code" }
        ]
      },
      {
        title: "Companion",
        rows: [
          { label: "Status", value: status.companionStatus, tone: this.statusTone(status.companionStatus), variant: "badge" },
          { label: "API", value: status.companionApiUrl ?? "None", variant: "code" },
          { label: "Discovery File", value: status.companionDiscoveryPath ?? "Not configured", variant: "code" },
          { label: "Detail", value: status.companionMessage ?? "None" }
        ]
      },
      {
        title: "Session",
        rows: [
          { label: "Microphone", value: status.microphonePermission, tone: this.statusTone(status.microphonePermission), variant: "badge" },
          { label: "Audio Input", value: status.selectedAudioInput },
          { label: "Recording", value: status.recording, tone: this.statusTone(status.recording), variant: "badge" },
          { label: "Current Meeting", value: status.currentMeetingTitle ?? "None" },
          { label: "Chunk Queue", value: `${status.pendingChunkCount} pending` },
          { label: "Last Transcript", value: this.formatLastTranscript(status.lastTranscriptAt) }
        ]
      }
    ];

    const list = container.createDiv({ cls: "echonote-status-list" });
    for (const section of sections) {
      const sectionEl = list.createDiv({ cls: "echonote-status-section" });
      sectionEl.createEl("h3", { text: section.title });
      for (const row of section.rows) {
        const item = sectionEl.createDiv({ cls: "echonote-status-row" });
        item.createSpan({ cls: "echonote-status-label", text: `${row.label}:` });
        item.createSpan({ cls: this.statusValueClass(row), text: this.formatRowValue(row) });
      }
    }

    if (status.lastError) {
      const error = container.createDiv({ cls: "echonote-status-error" });
      error.createEl("h3", { text: status.lastError.code });
      error.createEl("p", { text: status.lastError.message });
      if (status.lastError.detail) {
        error.createEl("pre", { text: status.lastError.detail });
      }
    }

    const actions = container.createDiv({ cls: "echonote-status-actions" });
    const utilityActions = actions.createDiv({ cls: "echonote-status-action-group" });
    this.addPanelAction(utilityActions, "Mic Permission", "mic", () => this.plugin.requestMicrophonePermission());
    this.addPanelAction(utilityActions, "Refresh", "refresh-cw", () => this.plugin.refreshCompanionStatus());
    this.addPanelAction(utilityActions, "Prepare ASR", "wrench", () => this.plugin.prepareCompanionAsr());

    const recordingActions = actions.createDiv({ cls: "echonote-status-action-group" });
    this.addPanelAction(recordingActions, "Start", "play", () => this.plugin.startMeeting(), "primary");
    this.addPanelAction(recordingActions, "Pause", "pause", () => this.plugin.pauseRecording());
    this.addPanelAction(recordingActions, "Resume", "rotate-ccw", () => this.plugin.resumeRecording());
    this.addPanelAction(recordingActions, "Stop", "square", () => this.plugin.stopMeeting(), "danger");
    this.addPanelAction(recordingActions, "Summarize", "sparkles", () => this.plugin.summarizeCurrentMeeting());
  }

  private addStatusPill(parent: HTMLElement, value: string): void {
    parent.createSpan({
      cls: `echonote-status-pill echonote-status-badge ${this.statusTone(value)}`,
      text: this.formatStatusText(value)
    });
  }

  private addSummaryItem(parent: HTMLElement, item: StatusSummaryItem): void {
    const tile = parent.createDiv({ cls: `echonote-status-summary-item ${item.tone}` });
    tile.createSpan({ cls: "echonote-status-summary-label", text: item.label });
    tile.createSpan({ cls: "echonote-status-summary-value", text: this.formatStatusText(item.value) });
  }

  private addPanelAction(parent: HTMLElement, text: string, icon: string, onClick: () => void, tone?: "primary" | "danger"): void {
    const button = parent.createEl("button", { cls: tone ? `echonote-status-action-${tone}` : "" });
    button.type = "button";
    button.setAttribute("aria-label", text);
    button.setAttribute("title", text);
    const iconEl = button.createSpan({ cls: "echonote-status-action-icon" });
    setIcon(iconEl, icon);
    button.createSpan({ cls: "echonote-status-action-label", text });
    button.addEventListener("click", onClick);
  }

  private statusValueClass(row: StatusRow): string {
    const classes = ["echonote-status-value"];
    if (row.tone) {
      classes.push(row.tone);
    }
    if (row.variant === "badge") {
      classes.push("echonote-status-badge");
    }
    if (row.variant === "code") {
      classes.push("echonote-status-code");
    }
    return classes.join(" ");
  }

  private formatRowValue(row: StatusRow): string {
    if (row.variant === "badge") {
      return this.formatStatusText(row.value);
    }
    return row.value;
  }

  private statusTone(value: string): string {
    if (["available", "granted", "ready", "recording", "running"].includes(value)) {
      return "is-success";
    }
    if (["denied", "error", "invalid", "missing", "stale", "unavailable"].includes(value)) {
      return "is-danger";
    }
    if (["loading", "starting", "stopping", "paused", "not_running"].includes(value)) {
      return "is-warning";
    }
    return "is-muted";
  }

  private formatStatusText(value: string): string {
    return value
      .replaceAll("_", " ")
      .replace(/\w\S*/g, (word) => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase());
  }

  private formatAsrRuntime(status: EchoNoteStatus): string {
    if (status.activeAsrRuntime === "unknown") {
      return this.formatStatusText(status.asrRuntime);
    }

    if (status.asrRuntime === status.activeAsrRuntime) {
      return this.formatStatusText(status.activeAsrRuntime);
    }

    return `${this.formatStatusText(status.asrRuntime)} -> ${this.formatStatusText(status.activeAsrRuntime)}`;
  }

  private formatLastTranscript(lastTranscriptAt: number | null): string {
    if (!lastTranscriptAt) {
      return "Never";
    }

    const secondsAgo = Math.max(0, Math.round((Date.now() - lastTranscriptAt) / 1000));
    return `${secondsAgo} seconds ago`;
  }
}
