export class CoalescingBatchWriter<T> {
  private pending: T[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private writeChain: Promise<void> = Promise.resolve();

  constructor(
    private readonly maxDelayMs: number,
    private readonly writeBatch: (items: T[]) => Promise<void>,
    private readonly onError: (error: unknown) => void
  ) {}

  enqueue(item: T): void {
    this.pending.push(item);
    if (this.timer) {
      return;
    }
    this.timer = setTimeout(() => {
      this.timer = null;
      this.queuePendingBatch();
    }, this.maxDelayMs);
  }

  async drain(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.queuePendingBatch();
    await this.writeChain;
  }

  private queuePendingBatch(): void {
    if (this.pending.length === 0) {
      return;
    }
    const batch = this.pending.splice(0);
    this.writeChain = this.writeChain
      .then(() => this.writeBatch(batch))
      .catch((error) => this.onError(error));
  }
}
