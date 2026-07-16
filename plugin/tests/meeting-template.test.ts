import test from "node:test";
import assert from "node:assert/strict";
import { renderMeetingTemplate, ECHONOTE_MEETING_MARKER } from "../src/meeting/meeting-template";
import { DEFAULT_SETTINGS } from "../src/settings/settings";

test("renderMeetingTemplate adds a stable EchoNote meeting marker", () => {
  const rendered = renderMeetingTemplate(DEFAULT_SETTINGS, {
    title: "2026-07-16 09-00 Meeting",
    startTime: new Date(2026, 6, 16, 9, 0),
    asrModel: "test-model",
    llmProvider: "openai-compatible"
  });

  assert.equal(rendered.startsWith(`${ECHONOTE_MEETING_MARKER}\n`), true);
  assert.match(rendered, /^# 2026-07-16 09-00 Meeting$/m);
});

test("renderMeetingTemplate does not duplicate an existing marker", () => {
  const rendered = renderMeetingTemplate({
    ...DEFAULT_SETTINGS,
    meetingTemplate: `${ECHONOTE_MEETING_MARKER}\n# {{meeting_title}}`
  }, {
    title: "Meeting",
    startTime: new Date(2026, 6, 16),
    asrModel: "test-model",
    llmProvider: "openai-compatible"
  });

  assert.equal(rendered.match(/<!-- echonote-meeting -->/g)?.length, 1);
});

test("renderMeetingTemplate keeps YAML frontmatter at the start of the note", () => {
  const rendered = renderMeetingTemplate({
    ...DEFAULT_SETTINGS,
    meetingTemplate: `---
type: meeting
---
# {{meeting_title}}

## Transcript`
  }, {
    title: "Meeting",
    startTime: new Date(2026, 6, 16),
    asrModel: "test-model",
    llmProvider: "openai-compatible"
  });

  assert.match(rendered, /^---\ntype: meeting\n---\n<!-- echonote-meeting -->\n/);
});
