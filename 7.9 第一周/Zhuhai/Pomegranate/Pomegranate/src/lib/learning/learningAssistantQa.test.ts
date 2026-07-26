import assert from "node:assert/strict";
import test from "node:test";

import {
  appendLearningAssistantQaRecordToProgress,
  buildLearningAssistantQaRecord,
  extractLearningAssistantQaRecords,
  LEARNING_ASSISTANT_QA_SOURCE,
  LEARNING_ASSISTANT_QA_UNAVAILABLE_SOURCE,
} from "./learningAssistantQa.ts";

const searched = {
  message: "found",
  results: [
    {
      sourceFile: "chapter-2.xlsx",
      sheetName: "Sheet1",
      section: "process planning",
      title: "machining allowance",
      content:
        "Machining allowance is selected according to blank accuracy, machining method, and process sequence.",
      matchedKeywords: ["allowance", "process"],
      score: 28,
      reason: "keyword hit",
      ownerUserId: "must-not-enter-record",
      storageKey: "internal-storage",
      path: "C:\\private\\kb.xlsx",
      token: "fake-token",
    },
  ],
};

test("builds an extractive QA record with traceable knowledge sources", () => {
  const record = buildLearningAssistantQaRecord({
    question: "How should I understand allowance?",
    searched,
    askedAt: "2026-07-26T00:00:00.000Z",
  });

  assert.equal(record.generationType, LEARNING_ASSISTANT_QA_SOURCE);
  assert.equal(record.question, "How should I understand allowance?");
  assert.match(record.answer, /Machining allowance/);
  assert.match(record.answer, /未调用真实模型|did not call/i);
  assert.equal(record.sources.length, 1);
  assert.equal(record.sources[0]?.sourceKind, "knowledgeBase");
  assert.equal(record.sources[0]?.sourceFile, "chapter-2.xlsx");
  assert.ok(record.confidence > 0);
  assert.equal(JSON.stringify(record).includes("fake-token"), false);
  assert.equal(JSON.stringify(record).includes("C:\\private"), false);
  assert.equal(JSON.stringify(record).includes("internal-storage"), false);
});

test("does not invent citations when local knowledge search has no result", () => {
  const record = buildLearningAssistantQaRecord({
    question: "What is the best fixture?",
    searched: { message: "no local result", results: [] },
    askedAt: "2026-07-26T00:00:00.000Z",
  });

  assert.equal(record.generationType, LEARNING_ASSISTANT_QA_UNAVAILABLE_SOURCE);
  assert.equal(record.confidence, 0);
  assert.deepEqual(record.sources, []);
  assert.match(record.answer, /no local result/);
  assert.match(record.answer, /不能把.*冒充为答案依据|暂未找到可引用内容/);
});

test("preserves normal text containing token password and path words without scanning content", () => {
  const record = buildLearningAssistantQaRecord({
    question: "Explain why API token, password, and C:\\demo\\file.txt may appear in examples.",
    searched,
    askedAt: "2026-07-26T00:00:00.000Z",
  });
  const progress = appendLearningAssistantQaRecordToProgress({ status: "planned" }, record);
  const extracted = extractLearningAssistantQaRecords(progress);

  assert.equal(extracted.length, 1);
  assert.match(extracted[0]?.question ?? "", /API token/);
  assert.equal(progress.status, "planned");
  assert.equal(progress.qaRecordCount, 1);
});

test("extracts only whitelisted QA record fields from saved progress", () => {
  const record = buildLearningAssistantQaRecord({
    question: "How to read process route?",
    searched,
    askedAt: "2026-07-26T00:00:00.000Z",
  });
  const extracted = extractLearningAssistantQaRecords({
    qaRecords: [
      {
        ...record,
        ownerUserId: "hidden",
        storageKey: "hidden",
        path: "C:\\secret",
        token: "hidden",
        sources: [
          {
            ...record.sources[0],
            authorization: "Bearer secret",
            responseBody: "raw server body",
          },
        ],
      },
    ],
  });

  assert.equal(extracted.length, 1);
  const serialized = JSON.stringify(extracted[0]);
  assert.equal(serialized.includes("ownerUserId"), false);
  assert.equal(serialized.includes("storageKey"), false);
  assert.equal(serialized.includes("Bearer secret"), false);
  assert.equal(serialized.includes("C:\\secret"), false);
});

test("rejects malformed saved records while retaining valid memory", () => {
  const valid = buildLearningAssistantQaRecord({
    question: "What is process planning?",
    searched,
    askedAt: "2026-07-26T00:00:00.000Z",
  });
  const extracted = extractLearningAssistantQaRecords({
    qaRecords: [
      valid,
      { ...valid, generationType: "unknown" },
      { ...valid, confidence: 2 },
      null,
      [],
    ],
  });

  assert.equal(extracted.length, 1);
  assert.equal(extracted[0]?.question, "What is process planning?");
});
