import test from "node:test";
import assert from "node:assert/strict";
import { CoalescingBatchWriter } from "../src/meeting/coalescing-batch-writer";

test("CoalescingBatchWriter combines items queued within the delay", async () => {
  const batches: number[][] = [];
  const writer = new CoalescingBatchWriter<number>(10, async (items) => {
    batches.push(items);
  }, assert.fail);

  writer.enqueue(1);
  writer.enqueue(2);
  writer.enqueue(3);
  await writer.drain();

  assert.deepEqual(batches, [[1, 2, 3]]);
});

test("CoalescingBatchWriter serializes writes and recovers after a failed batch", async () => {
  const events: string[] = [];
  const errors: unknown[] = [];
  let shouldFail = true;
  const writer = new CoalescingBatchWriter<number>(1, async (items) => {
    events.push(`start:${items.join(",")}`);
    if (shouldFail) {
      shouldFail = false;
      throw new Error("write failed");
    }
    events.push(`end:${items.join(",")}`);
  }, (error) => errors.push(error));

  writer.enqueue(1);
  await new Promise((resolve) => setTimeout(resolve, 5));
  writer.enqueue(2);
  await writer.drain();

  assert.equal(errors.length, 1);
  assert.deepEqual(events, ["start:1", "start:2", "end:2"]);
});
