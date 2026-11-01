// Debug smoke run (ECHONOTE_SMOKE, scripts/dev/app_smoke.sh): record the fake input through
// the real UI, check what was rendered and hand a report to Rust, which writes it and quits.

import { ai } from "./lib/ai.svelte";
import { app } from "./lib/app.svelte";
import { cspViolations } from "./lib/csp";
import { api, asCommandError, events } from "./lib/ipc";
import { models } from "./lib/models.svelte";
import type { MeetingDetail, ProviderInput } from "./lib/types";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

async function until(check: () => boolean, timeoutMs: number): Promise<boolean> {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    if (check()) return true;
    await sleep(100);
  }
  return check();
}

/// Clicks the third timestamp (muted) and checks the player lands there.
async function checkPlayback(detail: MeetingDetail): Promise<Record<string, unknown>> {
  const player = document.querySelector<HTMLAudioElement>(".detail audio.player");
  const buttons = document.querySelectorAll<HTMLButtonElement>(".detail .seek");
  const visible = detail.segments.filter((s) => s.crosstalk !== "duplicate" && s.text.trim());
  const result: Record<string, unknown> = { playerShown: !!player, seekButtons: buttons.length };
  const index = Math.min(2, buttons.length - 1);
  if (!player || index < 0) return result;
  player.muted = true;
  const expected = visible[index]!.startMs / 1000;
  const started = performance.now();
  buttons[index]!.click();
  result.landed = await until(() => !player.paused && Math.abs(player.currentTime - expected) < 1, 10_000);
  result.seekMs = Math.round(performance.now() - started);
  result.expectedS = expected;
  result.currentS = Math.round(player.currentTime * 100) / 100;
  result.durationS = Math.round(player.duration * 100) / 100;
  result.trackS = Math.max(...detail.tracks.map((track) => track.durationMs ?? 0)) / 1000;
  result.error = player.error?.code ?? null;
  player.pause();
  return result;
}

/// Edits the first line through the DOM, reverts it, then checks a glossary alias is shown
/// and searched as its term. Reports booleans only, no transcript text.
async function checkEditing(): Promise<Record<string, unknown>> {
  const result: Record<string, unknown> = {};
  const firstText = () => document.querySelector<HTMLElement>(".detail .row .text");
  const original = firstText()?.textContent?.trim() ?? "";
  if (!original) return result;
  firstText()!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  await sleep(100);
  const editor = document.querySelector<HTMLTextAreaElement>(".detail textarea.editor");
  result.editorOpened = !!editor;
  if (!editor) return result;
  editor.value = "冒烟测试修改";
  editor.dispatchEvent(new Event("input", { bubbles: true }));
  editor.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  result.edited = await until(() => !!firstText()?.textContent?.includes("冒烟测试修改"), 5_000);
  result.editedTag = !!document.querySelector(".detail .row .tag");
  document.querySelector<HTMLButtonElement>(".detail .row .revert")?.click();
  result.reverted = await until(() => firstText()?.textContent?.trim() === original, 5_000);

  const alias = [...original].slice(0, 2).join("");
  const term = "SMOKETERM";
  const saved = await api.saveGlossaryEntry({ id: null, term, aliases: [alias] });
  result.glossaryShown = await until(() => !!firstText()?.textContent?.startsWith(term), 5_000);
  const hits = await api.search(term);
  result.glossarySearch = hits.length > 0 && hits.every((hit) => hit.text.includes(term));
  for (const entry of saved) await api.deleteGlossaryEntry(entry.id);
  result.glossaryRemoved = await until(() => firstText()?.textContent?.trim() === original, 5_000);
  return result;
}

/// Exports both formats (the debug build writes them to ECHONOTE_EXPORT_DIR without the
/// dialog; the shell script checks the files), then deletes the meeting through the UI.
async function checkExportAndDelete(meeting: number): Promise<Record<string, unknown>> {
  const result: Record<string, unknown> = {};
  const offset = -new Date().getTimezoneOffset();
  const markdown = await api.exportMeeting(meeting, "markdown", offset);
  const text = await api.exportMeeting(meeting, "text", offset);
  result.exported = [markdown, text].map((path) => path?.split(/[\\/]/).pop() ?? null);
  const before = (await api.storageUsage()).audioBytes;
  document.querySelector<HTMLButtonElement>(".detail .delete")?.click();
  await sleep(100);
  const confirm = document.querySelector<HTMLButtonElement>(".detail .confirm-delete");
  result.confirmShown = !!confirm;
  confirm?.click();
  result.deleted = await until(
    () => app.view.name === "home" && !app.meetings.some((m) => m.id === meeting),
    5_000,
  );
  result.freedBytes = before - (await api.storageUsage()).audioBytes;
  result.gone = await api.getMeeting(meeting).then(
    () => false,
    (error) => asCommandError(error).code === "not-found",
  );
  return result;
}

function provider(baseUrl: string, model: string, apiKey: string | null, id: string | null = null): ProviderInput {
  return {
    id,
    name: "Smoke",
    preset: "custom",
    api: "openAi",
    baseUrl,
    model,
    contextTokens: 32_000,
    maxOutputTokens: 2_048,
    proxy: null,
    allowHttp: false,
    apiKey,
    headers: [],
  };
}

/// Sets up the mock server as the provider (a wrong key must fail first) and turns on live
/// highlights before recording.
async function checkAiSetup(baseUrl: string): Promise<Record<string, unknown>> {
  const result: Record<string, unknown> = {};
  result.badKey = await api.testProvider(provider(baseUrl, "smoke-model", "wrong")).then(
    () => "passed",
    (error) => asCommandError(error).code,
  );
  await api.testProvider(provider(baseUrl, "smoke-model", "smoke-key"));
  result.tested = true;
  result.models = (await api.listModels(provider(baseUrl, "smoke-model", "smoke-key"))).length;
  ai.settings = await api.saveProvider(provider(baseUrl, "smoke-model", "smoke-key"));
  result.active = ai.active?.model ?? null;
  result.local = ai.active?.local ?? null;
  await ai.setLiveHighlights(true, null);
  result.liveHighlights = ai.settings?.liveHighlights ?? false;
  return result;
}

const tab = (index: number) => document.querySelectorAll<HTMLButtonElement>(".detail [role=tab]")[index];

/// Notes (with the automatic title and a citation that seeks), a cancelled run, a question,
/// and LLM correction with keep and undo. Booleans and counts only.
async function checkAi(meeting: number, baseUrl: string): Promise<Record<string, unknown>> {
  const result: Record<string, unknown> = {};
  tab(0)?.click();
  await sleep(200);
  document.querySelector<HTMLButtonElement>(".detail .generate")?.click();
  result.notesShown = await until(() => !!document.querySelector(".detail .notes:not(.streaming) h3"), 20_000);
  result.noteCitations = document.querySelectorAll(".detail .notes .cite").length;
  const detail = await api.getMeeting(meeting);
  result.notesSaved = !!detail.notes;
  result.titled = detail.meeting.title === "冒烟测试会议";
  const player = document.querySelector<HTMLAudioElement>(".detail audio.player");
  const cite = document.querySelector<HTMLButtonElement>(".detail .notes button.cite");
  if (player && cite) {
    player.muted = true;
    cite.click();
    result.citationPlays = await until(() => !player.paused && player.currentTime > 0, 10_000);
    player.pause();
  }

  // A slow model, stopped while it streams; the saved notes stay.
  const id = ai.active!.id;
  ai.settings = await api.saveProvider(provider(baseUrl, "slow", null, id));
  document.querySelector<HTMLButtonElement>(".detail .generate")?.click();
  result.streamed = await until(() => !!document.querySelector(".detail .notes.streaming"), 10_000);
  await until(() => !!document.querySelector<HTMLButtonElement>(".detail .stop-notes:not([disabled])"), 5_000);
  document.querySelector<HTMLButtonElement>(".detail .stop-notes")?.click();
  result.cancelled = await until(() => ai.run(meeting, "notes") == null, 10_000);
  result.notesKept = !!document.querySelector(".detail .notes:not(.streaming) h3");
  ai.settings = await api.saveProvider(provider(baseUrl, "smoke-model", null, id));

  tab(2)?.click();
  await sleep(200);
  const input = document.querySelector<HTMLInputElement>(".detail .question-input");
  if (input) {
    input.value = "冒烟测试问题";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await sleep(50);
    document.querySelector<HTMLButtonElement>(".detail .ask-button")?.click();
    result.answerShown = await until(
      () => !!document.querySelector(".detail .turn:not(.pending) .answer button.cite"),
      20_000,
    );
    result.qaSaved = (await api.getMeeting(meeting)).qa.length;
  }

  tab(1)?.click();
  await sleep(200);
  document.querySelector<HTMLButtonElement>(".detail .run-correct")?.click();
  result.corrected = await until(() => document.querySelectorAll(".detail .row .corrected ins").length > 0, 20_000);
  result.pending = (await api.getMeeting(meeting)).corrections.length;
  document.querySelector<HTMLButtonElement>(".detail .row .keep")?.click();
  await until(() => document.querySelectorAll(".detail .row .corrected").length === (result.pending as number) - 1, 5_000);
  document.querySelector<HTMLButtonElement>(".detail .row .undo")?.click();
  await until(() => document.querySelectorAll(".detail .row .corrected").length === (result.pending as number) - 2, 5_000);
  const after = await api.getMeeting(meeting);
  result.leftToReview = after.corrections.length;
  result.kept = after.segments.filter((segment) => segment.text.endsWith("SMOKEFIX")).length;
  result.highlightsSaved = !!after.highlights;
  return result;
}

/// Model downloads from the local mirror: the VAD completes; the high-accuracy model is
/// paused part way and its partial download deleted; a required model cannot be deleted.
async function checkModels(): Promise<Record<string, unknown>> {
  const result: Record<string, unknown> = {};
  app.view = { name: "settings" };
  result.settingsShown = await until(() => !!document.querySelector(".settings .accurate-model"), 5_000);
  const vad = models.list.find((model) => model.role === "vad");
  const accurate = models.accurate;
  if (!vad || !accurate) return result;
  let progressEvents = 0;
  const stop = await events.models((event) => {
    if (event.event === "progress") progressEvents++;
  });
  await models.download(vad.id);
  result.vadDownloaded = await until(
    () => !models.downloading(vad.id) && !!models.list.find((model) => model.id === vad.id)?.removable,
    30_000,
  );
  result.vadError = models.errors[vad.id]?.code ?? null;
  result.requiredRefused = await api.deleteModel(vad.id).then(
    () => false,
    (error) => asCommandError(error).code === "model-required",
  );

  await models.download(accurate.id);
  await until(() => (models.progress[accurate.id]?.done ?? 0) > 0, 30_000);
  await models.cancel(accurate.id);
  result.paused = await until(() => !models.downloading(accurate.id), 10_000);
  const partial = models.accurate?.partial ?? 0;
  result.partialKept = partial > 0 && partial < accurate.size;
  result.pauseError = models.errors[accurate.id]?.code ?? null;
  await models.remove(accurate.id);
  result.partialDeleted = (models.accurate?.partial ?? -1) === 0;
  result.progressEvents = progressEvents;
  stop();
  return result;
}

/// The high-accuracy pass over the meeting (the dev data's Qwen3-ASR), then undo. Reports
/// counts and booleans only.
async function checkRetranscribe(meeting: number): Promise<Record<string, unknown>> {
  const result: Record<string, unknown> = { modelInstalled: models.accurate?.installed ?? false };
  app.view = { name: "meeting", id: meeting, focus: null };
  await until(() => !!document.querySelector(".detail .tabs"), 5_000);
  tab(1)?.click();
  await sleep(200);
  const button = document.querySelector<HTMLButtonElement>(".detail .retranscribe");
  result.buttonEnabled = !!button && !button.disabled;
  if (!button || button.disabled) return result;
  const before = (await api.getMeeting(meeting)).segments.map((segment) => segment.text);
  const started = performance.now();
  button.click();
  result.finished = await until(() => ai.run(meeting, "retranscribe")?.status !== "running", 180_000);
  result.seconds = Math.round((performance.now() - started) / 100) / 10;
  const run = ai.run(meeting, "retranscribe");
  result.status = run?.status ?? null;
  result.error = run?.error?.code ?? null;
  result.outcome = run?.result ?? null;
  const after = await api.getMeeting(meeting);
  result.retranscribed = after.retranscribed;
  result.changedText = after.segments.filter((segment, i) => segment.text !== before[i]).length;
  result.undoShown = await until(() => !!document.querySelector(".detail .undo-retranscribe"), 5_000);
  document.querySelector<HTMLButtonElement>(".detail .undo-retranscribe")?.click();
  result.undone = await until(() => !document.querySelector(".detail .undo-retranscribe"), 10_000);
  const restored = (await api.getMeeting(meeting)).segments.map((segment) => segment.text);
  result.textRestored = restored.length === before.length && restored.every((text, i) => text === before[i]);
  ai.clear(meeting, "retranscribe");
  return result;
}

/// The next frame, or after 1 s when the window is hidden and WebKit paused frames.
function frame(): Promise<{ at: number; painted: boolean }> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve({ at: performance.now(), painted: false }), 1_000);
    requestAnimationFrame((at) => {
      clearTimeout(timer);
      resolve({ at, painted: true });
    });
  });
}

/// Frame gaps while the list in `selector` scrolls from top to bottom in 60 steps, then
/// back to where it was. `painted` is false when the window was hidden (no frames to time).
async function scrollFrames(
  selector: string,
): Promise<{ rows: number; p95Ms: number; maxMs: number; painted: boolean } | null> {
  const viewport = document.querySelector<HTMLElement>(selector);
  if (!viewport) return null;
  const back = viewport.scrollTop;
  const range = viewport.scrollHeight - viewport.clientHeight;
  const gaps: number[] = [];
  let last = await frame();
  let painted = last.painted;
  for (let step = 0; step <= 60 && painted; step++) {
    viewport.scrollTop = (range * step) / 60;
    const now = await frame();
    painted = now.painted;
    gaps.push(now.at - last.at);
    last = now;
  }
  viewport.scrollTop = back;
  gaps.sort((a, b) => a - b);
  return {
    rows: viewport.querySelectorAll(".row").length,
    p95Ms: gaps.length ? Math.round(gaps[Math.floor(gaps.length * 0.95)]! * 10) / 10 : 0,
    maxMs: gaps.length ? Math.round(gaps.at(-1)! * 10) / 10 : 0,
    painted,
  };
}

/// PRD A6 (`scripts/dev/soak.sh`): record the looping fake input for `seconds`, scrolling the
/// live list every minute; then stop, time until everything is stored, and scroll the saved
/// transcript. Memory is sampled from outside by the script.
async function soak(seconds: number): Promise<Record<string, unknown>> {
  const report: Record<string, unknown> = { startedAt: new Date().toISOString(), seconds };
  const samples: { minute: number; segments: number; rows: number; p95Ms: number; maxMs: number; painted: boolean }[] =
    [];
  try {
    await api.startRecording();
    await until(() => app.view.name === "live" && (app.live?.segments.length ?? 0) > 0, 120_000);
    const started = performance.now();
    while (performance.now() - started < seconds * 1000) {
      await sleep(Math.min(60_000, seconds * 1000 - (performance.now() - started)));
      const frames = await scrollFrames(".live .viewport");
      const minute = Math.round((performance.now() - started) / 60_000);
      if (frames) samples.push({ minute, segments: app.live?.segments.length ?? 0, ...frames });
    }
    report.live = samples;
    report.visibility = document.visibilityState;
    report.segmentsAtStop = app.live?.segments.length ?? 0;
    const stopping = performance.now();
    await api.stopRecording();
    report.stopToStoredMs = Math.round(performance.now() - stopping);
    report.finished = await until(() => app.view.name === "meeting", 60_000);
    const meeting = app.live?.meeting ?? null;
    if (meeting != null) {
      const detail = await api.getMeeting(meeting);
      report.segments = detail.segments.length;
      report.status = detail.meeting.status;
      report.tracks = detail.tracks.map((track) => ({ kind: track.kind, durationMs: track.durationMs }));
      await until(() => !!document.querySelector(".detail .viewport .row"), 10_000);
      report.detailScroll = await scrollFrames(".detail .viewport");
    }
    report.finishedStats = app.live?.finished ?? null;
  } catch (error) {
    report.error = asCommandError(error);
  }
  report.cspViolations = [...cspViolations];
  return report;
}

export async function run(): Promise<void> {
  const soakSeconds = app.info?.soakSeconds ?? null;
  if (soakSeconds != null) {
    await api.smokeReport(await soak(soakSeconds));
    return;
  }
  const report: Record<string, unknown> = { startedAt: new Date().toISOString() };
  const deltaTimes: number[] = [];
  const unlisten = await events.live(() => deltaTimes.push(performance.now()));
  const llm = app.info?.smokeLlm ?? null;
  try {
    if (llm) report.aiSetup = await checkAiSetup(llm);
    const started = performance.now();
    await api.startRecording();
    report.startMs = Math.round(performance.now() - started);
    report.liveViewShown = await until(() => app.view.name === "live" && !!document.querySelector(".live"), 5_000);
    report.firstSegment = await until(() => (app.live?.segments.length ?? 0) > 0, 60_000);
    report.renderedRowsDuringRecording = document.querySelectorAll(".live .row").length;
    report.levelsSeen = (app.live?.levels ?? [0, 0]).some((level) => level > 0) || deltaTimes.length > 0;
    if (llm) {
      report.liveHighlightsShown = await until(
        () => !!document.querySelector(".live .highlights .bullets li"),
        60_000,
      );
    }
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
      report.playback = await checkPlayback(detail);
      report.editing = await checkEditing();
      if (llm) report.ai = await checkAi(meeting, llm);
      if (app.info?.smokeMirror) report.models = await checkModels();
      report.retranscribe = await checkRetranscribe(meeting);
      report.exportDelete = await checkExportAndDelete(meeting);
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
