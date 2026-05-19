import { ChildProcessWithoutNullStreams, spawn } from "child_process";
import { isAbsolute, resolve } from "path";
import { FileSystemAdapter, Notice, Platform, Plugin } from "obsidian";
import type { EchoNoteSettings } from "../settings/settings";

type ProcessLogHandler = (line: string) => void;
type ProcessExitHandler = (code: number | null, signal: NodeJS.Signals | null) => void;

export class AsrProcessManager {
  private process: ChildProcessWithoutNullStreams | null = null;
  private readonly recentLogs: string[] = [];

  constructor(
    private readonly plugin: Plugin,
    private readonly onLog: ProcessLogHandler,
    private readonly onExit: ProcessExitHandler
  ) {}

  isManagedProcessRunning(): boolean {
    return this.process !== null && this.process.exitCode === null;
  }

  start(settings: EchoNoteSettings, modelId: string): void {
    if (this.isManagedProcessRunning()) {
      return;
    }

    if (!Platform.isMacOS) {
      throw new Error("EchoNote ASR service startup is only supported on macOS in MVP.");
    }

    const asrServiceDir = this.resolveAsrServiceDir(settings);
    const args = [
      "-m",
      "echonote_asr",
      "--host",
      "127.0.0.1",
      "--port",
      String(settings.asrServicePort),
      "--model",
      modelId,
      "--backend",
      "fake",
      "--log-level",
      "info"
    ];

    this.process = spawn(settings.pythonPath, args, {
      cwd: asrServiceDir,
      env: {
        ...process.env,
        PYTHONPATH: asrServiceDir
      }
    });

    this.process.stdout.on("data", (data: Buffer) => this.handleLog(data));
    this.process.stderr.on("data", (data: Buffer) => this.handleLog(data));
    this.process.on("exit", (code, signal) => {
      this.process = null;
      this.onExit(code, signal);
    });
    this.process.on("error", (error) => {
      this.handleLog(Buffer.from(error.message));
      new Notice(`EchoNote ASR service failed to start: ${error.message}`);
    });
  }

  async stop(): Promise<void> {
    if (!this.process) {
      return;
    }

    const child = this.process;
    this.process = null;
    child.kill();
  }

  getRecentLogs(): string[] {
    return [...this.recentLogs];
  }

  private handleLog(data: Buffer): void {
    const text = data.toString("utf8").trim();
    if (!text) {
      return;
    }

    for (const line of text.split(/\r?\n/)) {
      this.recentLogs.push(line);
      if (this.recentLogs.length > 200) {
        this.recentLogs.shift();
      }
      this.onLog(line);
    }
  }

  private resolveAsrServiceDir(settings: EchoNoteSettings): string {
    if (isAbsolute(settings.asrServicePath)) {
      return settings.asrServicePath;
    }

    const adapter = this.plugin.app.vault.adapter;
    if (adapter instanceof FileSystemAdapter && this.plugin.manifest.dir) {
      return resolve(adapter.getBasePath(), this.plugin.manifest.dir, settings.asrServicePath);
    }

    return resolve(settings.asrServicePath);
  }
}
