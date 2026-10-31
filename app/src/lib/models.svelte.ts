// Speech models from the built-in manifest: what is installed, and downloads in progress.
// Downloads run in the app, so they continue while Settings is closed.

import { app } from "./app.svelte";
import { api, asCommandError, events } from "./ipc";
import type { CommandError, ModelEvent, ModelInfo, ModelProgress } from "./types";

class ModelStore {
  list = $state.raw<ModelInfo[]>([]);
  progress = $state.raw<Record<string, ModelProgress>>({});
  errors = $state.raw<Record<string, CommandError>>({});

  async init(): Promise<void> {
    await events.models((event) => this.onEvent(event));
    await this.load();
  }

  async load(): Promise<void> {
    try {
      this.list = await api.asrModels();
    } catch (error) {
      app.showError(asCommandError(error));
    }
  }

  get accurate(): ModelInfo | null {
    return this.list.find((model) => model.role === "accurate") ?? null;
  }

  downloading(id: string): boolean {
    return this.list.find((model) => model.id === id)?.downloading ?? false;
  }

  async download(id: string): Promise<void> {
    this.setError(id, null);
    this.mark(id, true);
    try {
      await api.downloadModel(id);
    } catch (error) {
      this.mark(id, false);
      this.setError(id, asCommandError(error));
    }
  }

  async cancel(id: string): Promise<void> {
    await api.cancelDownload(id).catch((error) => app.showError(asCommandError(error)));
  }

  async remove(id: string): Promise<void> {
    try {
      await api.deleteModel(id);
    } catch (error) {
      app.showError(asCommandError(error));
    }
    this.setError(id, null);
    await this.load();
  }

  private mark(id: string, downloading: boolean): void {
    this.list = this.list.map((model) => (model.id === id ? { ...model, downloading } : model));
  }

  private setError(id: string, error: CommandError | null): void {
    const errors = { ...this.errors };
    if (error) errors[id] = error;
    else delete errors[id];
    this.errors = errors;
  }

  private onEvent(event: ModelEvent): void {
    if (event.event === "progress") {
      this.progress = { ...this.progress, [event.id]: { stage: event.stage, done: event.done, total: event.total } };
      if (!this.downloading(event.id)) this.mark(event.id, true);
      return;
    }
    const progress = { ...this.progress };
    delete progress[event.id];
    this.progress = progress;
    if (event.event === "error" && event.error.code !== "cancelled") this.setError(event.id, event.error);
    void this.load();
    // A repaired live model or VAD shows as installed.
    if (event.event === "done") void api.appInfo().then((info) => (app.info = info)).catch(() => {});
  }
}

export const models = new ModelStore();
