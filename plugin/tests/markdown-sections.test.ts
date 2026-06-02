import test from "node:test";
import assert from "node:assert/strict";
import { extractTranscript, replaceMeetingEndTime, replaceTranscriptSection } from "../src/meeting/markdown-sections";

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
