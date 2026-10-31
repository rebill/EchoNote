// Debug smoke run (ECHONOTE_SMOKE, scripts/dev/app_smoke.sh): record the fake input through
// the real UI, check what was rendered and hand a report to Rust, which writes it and quits.

import { app } from "./lib/app.svelte";
import { cspViolations } from "./lib/csp";
import { api, asCommandError, events } from "./lib/ipc";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function until(check: () => boolean, timeoutMs: number): Promise<boolean> {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    if (check()) return true;
    await sleep(100);
  }
  return check();
}

export async function run(): Promise<void> {
  const report: Record<string, unknown> = { startedAt: new Date().toISOString() };
  const deltaTimes: number[] = [];
  const unlisten = await events.live(() => deltaTimes.push(performance.now()));
  try {
    const started = performance.now();
    await api.startRecording();
    report.startMs = Math.round(performance.now() - started);
    report.liveViewShown = await until(() => app.view.name === "live" && !!document.querySelector(".live"), 5_000);
    report.firstSegment = await until(() => (app.live?.segments.length ?? 0) > 0, 60_000);
    report.renderedRowsDuringRecording = document.querySelectorAll(".live .row").length;
    report.levelsSeen = (app.live?.levels ?? [0, 0]).some((level) => level > 0) || deltaTimes.length > 0;
    // The fake input ends on its own; the session finalizes when both files are played.
    report.finished = await until(() => app.view.name === "meeting", 10 * 60_000);
    const meeting = app.live?.meeting ?? null;
    report.meeting = meeting;
    await sleep(500);
    report.renderedRowsInDetail = document.querySelectorAll(".detail .row").length;
    report.speakerLabels = document.querySelectorAll(".detail .speaker").length > 0;
    if (meeting != null) {
      const detail = await api.getMeeting(meeting);
      report.segments = detail.segments.length;
      report.visibleSegments = detail.segments.filter((s) => s.crosstalk !== "duplicate" && s.text.trim()).length;
      report.hiddenSegments = detail.segments.filter((s) => s.crosstalk === "duplicate").length;
      report.tracks = detail.tracks.map((track) => ({ kind: track.kind, durationMs: track.durationMs }));
      report.status = detail.meeting.status;
    }
    const gaps = deltaTimes.slice(1).map((time, i) => time - deltaTimes[i]!);
    gaps.sort((a, b) => a - b);
    report.deltas = deltaTimes.length;
    report.deltasPerSecond =
      deltaTimes.length > 1 ? (deltaTimes.length - 1) / ((deltaTimes.at(-1)! - deltaTimes[0]!) / 1000) : 0;
    report.deltaGapP5Ms = gaps.length ? Math.round(gaps[Math.floor(gaps.length * 0.05)]!) : null;
    report.finishedStats = app.live?.finished ?? null;
  } catch (error) {
    report.error = asCommandError(error);
  } finally {
    unlisten();
  }
  report.cspViolations = [...cspViolations];
  report.toast = app.toast;
  await api.smokeReport(report);
}
