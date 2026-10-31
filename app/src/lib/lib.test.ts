import { describe, expect, it } from "vitest";

import { clock, duration } from "./format";
import { applyDelta, elapsed, fromSnapshot, hasOtherSpeech, insertSegment, liveRows, segmentRows } from "./live";
import { banners, deviceName } from "./notices";
import { en, zh } from "./strings";
import type { Delta, Segment } from "./types";
import { Layout, rowAt, visibleRange } from "./virtual";

function segment(id: number, track: "mic" | "sys", startMs: number, text = "x"): Segment {
  return { id, track, startMs, endMs: startMs + 500, text, crosstalk: null, source: "asr", revision: 0 };
}

function delta(meeting: number, fields: Partial<Delta> = {}): Delta {
  return {
    meeting,
    state: null,
    elapsedMs: 0,
    levels: null,
    previews: null,
    segments: [],
    notices: null,
    finished: null,
    ...fields,
  };
}

describe("strings", () => {
  it("has the same keys and placeholders in both languages", () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(zh).sort());
    const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    for (const key of Object.keys(zh) as (keyof typeof zh)[]) {
      expect(placeholders(en[key]), key).toEqual(placeholders(zh[key]));
    }
  });
});

describe("format", () => {
  it("formats clocks and durations", () => {
    expect(clock(0)).toBe("0:00");
    expect(clock(65_400)).toBe("1:05");
    expect(clock(3_725_000)).toBe("1:02:05");
    expect(duration(30_000, "zh-CN")).toBe("< 1 分钟");
    expect(duration(65 * 60_000, "en")).toBe("1 h 5 min");
    expect(duration(120 * 60_000, "zh-CN")).toBe("2 小时");
  });
});

describe("live view", () => {
  it("keeps segments in time order without duplicates", () => {
    const segments: Segment[] = [];
    insertSegment(segments, segment(2, "sys", 2_000));
    insertSegment(segments, segment(1, "mic", 1_000));
    insertSegment(segments, segment(3, "mic", 2_000));
    insertSegment(segments, segment(2, "sys", 2_000, "edited"));
    expect(segments.map((s) => s.id)).toEqual([1, 3, 2]);
    expect(segments.map((s) => [s.startMs, s.track])).toEqual([
      [1_000, "mic"],
      [2_000, "mic"],
      [2_000, "sys"],
    ]);
    expect(segments[2]!.text).toBe("edited");
  });

  it("merges deltas onto a snapshot and starts over for a new meeting", () => {
    let view = fromSnapshot(
      { meeting: 4, state: "recording", elapsedMs: 1_000, previews: ["", ""], notices: [], segments: [segment(1, "mic", 0)] },
      0,
    );
    // A segment can be both in the snapshot and in a delta that was in flight.
    view = applyDelta(view, delta(4, { elapsedMs: 1_100, levels: [0.3, 0.1], segments: [segment(1, "mic", 0), segment(2, "sys", 500)] }), 100);
    expect(view.segments.map((s) => s.id)).toEqual([1, 2]);
    expect(view.levels).toEqual([0.3, 0.1]);
    expect(elapsed(view, 600)).toBe(1_600);

    view = applyDelta(view, delta(4, { state: "paused", elapsedMs: 1_200, levels: [0.5, 0.5], previews: ["a", "b"] }), 200);
    expect(view.levels).toEqual([0, 0]);
    expect(view.previews).toEqual(["", ""]);
    expect(elapsed(view, 10_000)).toBe(1_200);

    const next = applyDelta(view, delta(5, { state: "recording" }), 300);
    expect(next.meeting).toBe(5);
    expect(next.segments).toEqual([]);
  });

  it("shows speaker labels only when the other side spoke", () => {
    expect(hasOtherSpeech([segment(1, "mic", 0)])).toBe(false);
    expect(hasOtherSpeech([segment(1, "mic", 0), segment(2, "sys", 0, " ")])).toBe(false);
    expect(hasOtherSpeech([segment(1, "mic", 0)], ["", "hi"])).toBe(true);
    expect(hasOtherSpeech([segment(2, "sys", 0)])).toBe(true);
  });
});

describe("transcript rows", () => {
  it("hides bleed unless asked and puts previews last", () => {
    const segments = [segment(1, "mic", 0), { ...segment(2, "mic", 600), crosstalk: "duplicate" as const }, segment(3, "sys", 700)];
    expect(segmentRows(segments).map((row) => row.key)).toEqual(["s1", "s3"]);
    const all = segmentRows(segments, true);
    expect(all.map((row) => [row.key, row.hidden])).toEqual([["s1", false], ["s2", true], ["s3", false]]);
    const view = fromSnapshot({ meeting: 1, state: "recording", elapsedMs: 5_000, previews: ["", "so then"], notices: [], segments }, 0);
    const rows = liveRows(view);
    expect(rows.at(-1)).toMatchObject({ key: "psys", preview: true, text: "so then", startMs: 1_200 });
  });
});

describe("virtual list", () => {
  it("uses measured heights and estimates the rest", () => {
    const layout = new Layout(50);
    expect(layout.set("a", 80)).toBe(true);
    expect(layout.set("a", 80)).toBe(false);
    expect(layout.offsets(["a", "b", "c"])).toEqual([0, 80, 130, 180]);
    layout.retain(["b"]);
    expect(layout.height("a")).toBe(50);
  });

  it("finds the rows in the viewport", () => {
    const offsets = new Layout(10).offsets(Array.from({ length: 1_000 }, (_, i) => String(i)));
    expect(rowAt(offsets, 0)).toBe(0);
    expect(rowAt(offsets, 95)).toBe(9);
    expect(rowAt(offsets, 1e9)).toBe(999);
    expect(visibleRange(offsets, 1_000, 100, 2)).toEqual([98, 112]);
    expect(visibleRange(offsets, 0, 100, 0)).toEqual([0, 10]);
    expect(visibleRange([0], 0, 100)).toEqual([0, 0]);
  });
});

describe("notices", () => {
  it("maps notices to banners and meter names", () => {
    const notices = [
      { kind: "modelFailed", error: "missing" },
      { kind: "sourceStarted", track: "mic", name: "Built-in" },
      { kind: "permissionDenied", track: "sys" },
      { kind: "bleed" },
    ] as const;
    expect(banners([...notices], true).map((b) => [b.key, b.text, b.settings])).toEqual([
      ["model", "notice.modelFailed", undefined],
      ["source-sys", "notice.sysDenied", "sys"],
      ["bleed", "notice.bleed", undefined],
    ]);
    // Without a model one banner says so; load errors are not repeated.
    expect(banners([...notices], false).filter((b) => b.key === "model").map((b) => b.text)).toEqual([
      "notice.modelMissing",
    ]);
    expect(deviceName([...notices], "mic")).toBe("Built-in");
    expect(deviceName([...notices], "sys")).toBeNull();
  });
});
