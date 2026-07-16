import test from "node:test";
import assert from "node:assert/strict";
import {
  extractTranscript,
  isEchoNoteMeetingNote,
  replaceDocumentTitle,
  replaceMeetingEndTime,
  replaceTranscriptSection
} from "../src/meeting/markdown-sections";

test("replaceTranscriptSection replaces only transcript content", () => {
  const markdown = `# Meeting

## Summary

_Pending._

## Transcript

[00:00:00] old
`;

  const updated = replaceTranscriptSection(markdown, "[00:00:00] Speaker 1: new");

  assert.equal(extractTranscript(updated), "[00:00:00] Speaker 1: new");
  assert.match(updated, /## Summary\n\n_Pending\._/);
});

test("replaceTranscriptSection inserts transcript section when missing", () => {
  const updated = replaceTranscriptSection("# Meeting\n", "[00:00:00] hello");

  assert.match(updated, /## Transcript/);
  assert.equal(extractTranscript(updated), "[00:00:00] hello");
});

test("replaceMeetingEndTime writes the stop time in the default metadata line", () => {
  const markdown = `# Meeting

- Date: 2026-06-02
- Time: 09:03:03 - 
- Platform: EchoNote

## Transcript

[00:00] hello
`;

  const updated = replaceMeetingEndTime(markdown, "09:08:42");

  assert.match(updated, /- Time: 09:03:03 - 09:08:42/);
  assert.equal(extractTranscript(updated), "[00:00] hello");
});

test("replaceDocumentTitle updates only the level-one document title", () => {
  const markdown = `# Old title

## Summary

Keep this section.
`;

  const updated = replaceDocumentTitle(markdown, "2026-07-13_产品复盘");

  assert.match(updated, /^# 2026-07-13_产品复盘$/m);
  assert.match(updated, /^## Summary$/m);
});

test("isEchoNoteMeetingNote accepts a default EchoNote meeting", () => {
  assert.equal(isEchoNoteMeetingNote(`# Meeting

- Platform: EchoNote
- Tags: #meeting #echonote

## Transcript
`), true);
});

test("isEchoNoteMeetingNote accepts the stable meeting marker", () => {
  assert.equal(isEchoNoteMeetingNote(`<!-- echonote-meeting -->
# Custom meeting

## Transcript
`), true);
});

test("isEchoNoteMeetingNote rejects an unrelated note with a transcript heading", () => {
  assert.equal(isEchoNoteMeetingNote(`# Research

## Transcript

Quoted interview text.
`), false);
});

test("isEchoNoteMeetingNote ignores EchoNote markers inside the transcript", () => {
  assert.equal(isEchoNoteMeetingNote(`# Research

## Transcript

Someone mentioned #echonote during the interview.
`), false);
});

test("isEchoNoteMeetingNote accepts legacy templates with all summary sections", () => {
  assert.equal(isEchoNoteMeetingNote(`# Meeting

## Summary
## Decisions
## Action Items
## Key Points
## Open Questions
## Transcript
`), true);
});
