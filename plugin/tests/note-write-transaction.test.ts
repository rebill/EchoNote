import test from "node:test";
import assert from "node:assert/strict";
import { renameThenWriteNote } from "../src/meeting/note-write-transaction";

type TestFile = { path: string };

test("renameThenWriteNote rolls the path back when writing fails", async () => {
  const file: TestFile = { path: "Meetings/old.md" };
  const renames: string[] = [];

  await assert.rejects(() => renameThenWriteNote({
    file,
    previousPath: "Meetings/old.md",
    targetPath: "Meetings/new.md",
    content: "updated",
    getPath: (item) => item.path,
    rename: async (item, path) => {
      item.path = path;
      renames.push(path);
      return item;
    },
    write: async () => {
      throw new Error("disk full");
    }
  }), /disk full/);

  assert.deepEqual(renames, ["Meetings/new.md", "Meetings/old.md"]);
  assert.equal(file.path, "Meetings/old.md");
});

test("renameThenWriteNote does not write when the initial rename fails", async () => {
  const file: TestFile = { path: "Meetings/old.md" };
  let writes = 0;

  await assert.rejects(() => renameThenWriteNote({
    file,
    previousPath: "Meetings/old.md",
    targetPath: "Meetings/new.md",
    content: "updated",
    getPath: (item) => item.path,
    rename: async () => {
      throw new Error("name collision");
    },
    write: async () => {
      writes += 1;
    }
  }), /name collision/);

  assert.equal(writes, 0);
  assert.equal(file.path, "Meetings/old.md");
});

test("renameThenWriteNote returns the renamed file after a successful write", async () => {
  const file: TestFile = { path: "Meetings/old.md" };
  let writtenContent = "";

  const result = await renameThenWriteNote({
    file,
    previousPath: "Meetings/old.md",
    targetPath: "Meetings/new.md",
    content: "updated",
    getPath: (item) => item.path,
    rename: async (item, path) => {
      item.path = path;
      return item;
    },
    write: async (_item, content) => {
      writtenContent = content;
    }
  });

  assert.equal(result.path, "Meetings/new.md");
  assert.equal(writtenContent, "updated");
});

test("renameThenWriteNote reports a failed rollback", async () => {
  const file: TestFile = { path: "Meetings/old.md" };

  await assert.rejects(() => renameThenWriteNote({
    file,
    previousPath: "Meetings/old.md",
    targetPath: "Meetings/new.md",
    content: "updated",
    getPath: (item) => item.path,
    rename: async (item, path) => {
      if (path === "Meetings/old.md") {
        throw new Error("rollback blocked");
      }
      item.path = path;
      return item;
    },
    write: async () => {
      throw new Error("disk full");
    }
  }), /rollback blocked/);
});
