import assert from "node:assert/strict";
import test from "node:test";

import {
  normalizeCreateMarkdownInput,
  safeCreateMarkdownShape,
} from "./createMarkdownInput.ts";

test("minimal create input receives every legal default", () => {
  assert.deepEqual(normalizeCreateMarkdownInput({}), {
    title: "未命名文档",
    markdownContent: "",
    folderId: null,
    diaryDate: null,
    isPinned: false,
    isHidden: false,
    sortOrder: 0,
    tagIds: [],
  });
});

test("blank title and missing Markdown are normalized", () => {
  const input = normalizeCreateMarkdownInput({ title: "   " });
  assert.equal(input.title, "未命名文档");
  assert.equal(input.markdownContent, "");
});

test("an invalid previous-account folder is represented as null by the repository boundary", () => {
  const input = normalizeCreateMarkdownInput({ folderId: null });
  assert.equal(input.folderId, null);
});

test("missing tags become an empty array and no field is undefined", () => {
  const input = normalizeCreateMarkdownInput({ tagIds: undefined });
  assert.deepEqual(input.tagIds, []);
  assert.equal(Object.values(input).includes(undefined), false);
});

test("safe diagnostics contain only field types and collection lengths", () => {
  assert.deepEqual(safeCreateMarkdownShape(normalizeCreateMarkdownInput({})), {
    title: "string",
    markdownContent: "string",
    folderId: "null",
    diaryDate: "null",
    isPinned: "boolean",
    isHidden: "boolean",
    sortOrder: "number",
    tagIds: "array(0)",
  });
});

test("wrong Markdown, folder, and tag shapes receive specific safe errors", () => {
  assert.throws(() => normalizeCreateMarkdownInput({ markdownContent: 42 }), { code: "markdownContentInvalid" });
  assert.throws(() => normalizeCreateMarkdownInput({ folderId: 42 }), { code: "folderInvalid" });
  assert.throws(() => normalizeCreateMarkdownInput({ tagIds: "bad" }), { code: "tagsInvalid" });
});
