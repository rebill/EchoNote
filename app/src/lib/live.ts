// The recording view's model: a snapshot plus the deltas that follow it. Pure functions, so
// the merge rules are unit tested (live.test.ts).

import type { Delta, Finished, Notice, Segment, SessionState, Snapshot, TranscriptRow } from "./types";

export interface LiveView {
  meeting: number;
  state: SessionState;
  elapsedMs: number;
  /// Clock time (performance.now()) when `elapsedMs` was current.
  elapsedAt: number;
  levels: [number, number];
  previews: [string, string];
  /// Committed finals in time order.
  segments: Segment[];
  notices: Notice[];
  finished: Finished | null;
}

export function fromSnapshot(snapshot: Snapshot, now: number): LiveView {
  const view = empty(snapshot.meeting, now);
  view.state = snapshot.state;
  view.elapsedMs = snapshot.elapsedMs;
  view.previews = snapshot.previews;
  view.notices = snapshot.notices;
  for (const segment of snapshot.segments) insertSegment(view.segments, segment);
  return view;
}

function empty(meeting: number, now: number): LiveView {
  return {
    meeting,
    state: "starting",
    elapsedMs: 0,
    elapsedAt: now,
    levels: [0, 0],
    previews: ["", ""],
    segments: [],
    notices: [],
    finished: null,
  };
}

function before(a: Segment, b: Segment): boolean {
  return a.startMs < b.startMs || (a.startMs === b.startMs && a.track < b.track);
}

/// Insert in time order; a segment already present (same id) is replaced.
export function insertSegment(segments: Segment[], segment: Segment): void {
  const existing = segments.findIndex((s) => s.id === segment.id);
  if (existing >= 0) {
    segments[existing] = segment;
    return;
  }
  // Finals mostly arrive in order: search from the end.
  let at = segments.length;
  while (at > 0 && before(segment, segments[at - 1]!)) at--;
  segments.splice(at, 0, segment);
}

/// Apply a delta. A delta for another meeting starts a new view (a new recording).
export function applyDelta(view: LiveView | null, delta: Delta, now: number): LiveView {
  const next = view && view.meeting === delta.meeting ? view : empty(delta.meeting, now);
  if (delta.state) next.state = delta.state;
  next.elapsedMs = delta.elapsedMs;
  next.elapsedAt = now;
  next.levels = next.state === "recording" && delta.levels ? delta.levels : [0, 0];
  if (delta.previews) next.previews = delta.previews;
  if (next.state !== "recording") next.previews = ["", ""];
  for (const segment of delta.segments) insertSegment(next.segments, segment);
  if (delta.notices) next.notices = delta.notices;
  if (delta.finished) next.finished = delta.finished;
  return next;
}

/// Recording time now: counts on between deltas while recording.
export function elapsed(view: LiveView, now: number): number {
  return view.state === "recording" ? view.elapsedMs + Math.max(0, now - view.elapsedAt) : view.elapsedMs;
}

export function isActive(view: LiveView | null): boolean {
  return !!view && view.state !== "finalized" && !view.finished;
}

/// Whether the other side said anything: without it the speaker labels are left out (PRD A4).
export function hasOtherSpeech(segments: Segment[], previews: [string, string] = ["", ""]): boolean {
  return previews[1].trim() !== "" || segments.some((s) => s.track === "sys" && s.text.trim() !== "");
}

/// Rows for a transcript: finals in time order; speaker bleed only when `showHidden`.
export function segmentRows(segments: Segment[], showHidden = false): TranscriptRow[] {
  const rows: TranscriptRow[] = [];
  for (const segment of segments) {
    const hidden = segment.crosstalk === "duplicate";
    if (hidden && !showHidden) continue;
    if (!segment.text.trim()) continue;
    rows.push({
      key: `s${segment.id}`,
      segmentId: segment.id,
      track: segment.track,
      startMs: segment.startMs,
      text: segment.text,
      preview: false,
      hidden,
    });
  }
  return rows;
}

/// Live rows: the finals, then the utterances in progress (mic, then sys).
export function liveRows(view: LiveView): TranscriptRow[] {
  const rows = segmentRows(view.segments);
  const tracks = ["mic", "sys"] as const;
  view.previews.forEach((text, index) => {
    if (!text.trim()) return;
    const track = tracks[index]!;
    const last = [...view.segments].reverse().find((segment) => segment.track === track);
    rows.push({
      key: `p${track}`,
      segmentId: null,
      track,
      startMs: Math.min(last?.endMs ?? 0, view.elapsedMs),
      text,
      preview: true,
      hidden: false,
    });
  });
  return rows;
}
