import { DEFAULT_STATUS, type EchoNoteStatus } from "./status-types";

export type StatusListener = (status: EchoNoteStatus) => void;

export class StatusStore {
  private status: EchoNoteStatus;
  private listeners = new Set<StatusListener>();

  constructor(initialStatus: Partial<EchoNoteStatus> = {}) {
    this.status = {
      ...DEFAULT_STATUS,
      ...initialStatus
    };
  }

  getState(): EchoNoteStatus {
    return this.status;
  }

  setState(nextStatus: Partial<EchoNoteStatus>): void {
    this.status = {
      ...this.status,
      ...nextStatus
    };

    this.emit();
  }

  subscribe(listener: StatusListener): () => void {
    this.listeners.add(listener);
    listener(this.status);

    return () => {
      this.listeners.delete(listener);
    };
  }

  private emit(): void {
    for (const listener of this.listeners) {
      listener(this.status);
    }
  }
}
