import assert from "node:assert/strict";
import test from "node:test";

import { buildLearningAssistantProgressOverview } from "./learningAssistantProgress.ts";

const plan = {
  stages: [
    { name: "基础诊断" },
    { name: "核心学习" },
    { name: "综合复盘" },
  ],
};

test("builds persisted project progress overview from quiz mastery and activities", () => {
  const overview = buildLearningAssistantProgressOverview({
    plan,
    linkedDocumentCount: 2,
    planAdjustments: [
      {
        adjustedAt: "2026-07-26T03:00:00.000Z",
        reason: "根据阶段测试追加薄弱点补学任务",
      },
    ],
    progress: {
      updatedAt: "2026-07-26T01:00:00.000Z",
      qaRecords: [
        {
          question: "工艺路线如何制定？",
          askedAt: "2026-07-26T02:00:00.000Z",
          token: "should-not-leak",
        },
      ],
      quizRecords: [
        {
          recordKey: "quiz-1",
          stageIndex: 0,
          stageName: "基础诊断",
          percentage: 86,
          testedAt: "2026-07-26T02:30:00.000Z",
          canAdvance: true,
          weakKnowledgePoints: [],
        },
        {
          recordKey: "quiz-2",
          stageIndex: 1,
          stageName: "核心学习",
          percentage: 42,
          testedAt: "2026-07-26T02:45:00.000Z",
          canAdvance: false,
          weakKnowledgePoints: ["工序集中"],
          storageKey: "should-not-leak",
        },
      ],
      masteryRecords: [
        { masteryLevel: "mastered" },
        { masteryLevel: "weak" },
        { masteryLevel: "basic" },
      ],
    },
  });

  assert.equal(overview.stageCount, 3);
  assert.equal(overview.completedStageCount, 1);
  assert.equal(overview.needsReviewStageCount, 1);
  assert.equal(overview.progressPercent, 33);
  assert.equal(overview.quizRecordCount, 2);
  assert.equal(overview.qaRecordCount, 1);
  assert.equal(overview.linkedDocumentCount, 2);
  assert.deepEqual(overview.mastery, { mastered: 1, basic: 1, weak: 1 });
  assert.deepEqual(
    overview.stageStatuses.map((stage) => stage.status),
    ["completed", "needsReview", "inProgress"],
  );
  assert.equal(overview.stageStatuses[1].weakKnowledgePoints[0], "工序集中");
  assert.equal(overview.recentActivities[0].activityType, "replan");
  assert.equal(JSON.stringify(overview).includes("should-not-leak"), false);
});

test("keeps empty and malformed progress safe without inventing completion", () => {
  const overview = buildLearningAssistantProgressOverview({
    plan: { stages: [{ name: "阶段一" }, { name: "阶段二" }] },
    linkedDocumentCount: -1,
    progress: {
      updatedAt: "not-a-date",
      quizRecords: [
        {
          recordKey: "bad",
          stageIndex: "0",
          percentage: 1000,
          testedAt: "2026-07-26T00:00:00.000Z",
        },
      ],
      masteryRecords: [{ masteryLevel: "unknown" }],
    },
    planAdjustments: [{ adjustedAt: "", reason: "ignored" }],
  });

  assert.equal(overview.completedStageCount, 0);
  assert.equal(overview.progressPercent, 0);
  assert.equal(overview.linkedDocumentCount, 0);
  assert.deepEqual(
    overview.stageStatuses.map((stage) => stage.status),
    ["inProgress", "notStarted"],
  );
  assert.deepEqual(overview.mastery, { mastered: 0, basic: 0, weak: 0 });
  assert.equal(overview.recentActivities.length, 0);
});
