import assert from "node:assert/strict";
import test from "node:test";

import {
  attachDiagnosisToUnderstanding,
  buildLearningAssistantDiagnosis,
  extractLearningAssistantDiagnosis,
  LEARNING_ASSISTANT_DIAGNOSIS_SOURCE,
} from "./learningAssistantDiagnostics.ts";

const goal = {
  courseName: "机械制造工艺学",
  learningGoal: "系统学习",
  learningCycle: "3周",
  dailyStudyHours: 1,
  currentLevel: "基础一般：掌握部分概念，但缺少系统复习",
  finalGoal: "梳理完整课程知识框架",
};

const plan = {
  stages: [
    {
      learningEntries: [
        {
          title: "工艺规程设计",
          masteryLevel: "理解",
          sourceType: "knowledgeBase",
          sourceFile: "chapter-1.xlsx",
        },
        {
          title: "定位基准选择",
          masteryLevel: "掌握",
          sourceType: "knowledgeBase",
          sourceFile: "chapter-2.xlsx",
        },
      ],
      knowledgePoints: ["工艺规程设计", "加工余量确定"],
    },
  ],
};

test("builds project-owned diagnosis from goal and knowledge-backed plan entries", () => {
  const diagnosis = buildLearningAssistantDiagnosis(goal, plan, "2026-07-26T00:00:00.000Z");

  assert.equal(diagnosis.source, LEARNING_ASSISTANT_DIAGNOSIS_SOURCE);
  assert.equal(diagnosis.generatedAt, "2026-07-26T00:00:00.000Z");
  assert.deepEqual(diagnosis.pendingKnowledgePoints, [
    "工艺规程设计",
    "定位基准选择",
    "加工余量确定",
  ]);
  assert.deepEqual(diagnosis.masteredKnowledgePoints, [
    "工艺规程设计",
    "定位基准选择",
  ]);
  assert.deepEqual(diagnosis.weakKnowledgePoints, ["加工余量确定"]);
  assert.ok(diagnosis.basis.includes("本地知识点计划条目"));
});

test("draft diagnosis stays honest before a plan has knowledge-point evidence", () => {
  const diagnosis = buildLearningAssistantDiagnosis(
    { ...goal, currentLevel: "零基础：基本没有学习过本课程" },
    null,
    "2026-07-26T00:00:00.000Z",
  );

  assert.deepEqual(diagnosis.pendingKnowledgePoints, []);
  assert.deepEqual(diagnosis.masteredKnowledgePoints, []);
  assert.deepEqual(diagnosis.weakKnowledgePoints, []);
  assert.ok(diagnosis.basis.includes("待生成计划后补充知识点证据"));
  assert.match(diagnosis.summary, /待诊断基础/);
});

test("diagnosis is attached and extracted through a safe whitelist shape", () => {
  const diagnosis = buildLearningAssistantDiagnosis(goal, plan, "2026-07-26T00:00:00.000Z");
  const understanding = attachDiagnosisToUnderstanding(
    {
      summary: "目标理解",
      currentGap: "当前差距",
      strategy: "学习策略",
      closedLoop: "闭环",
      ownerUserId: "should-remain-outside-rendered-diagnosis",
    },
    diagnosis,
  );

  const extracted = extractLearningAssistantDiagnosis(understanding);
  assert.deepEqual(extracted, diagnosis);
  assert.equal(extracted?.weakKnowledgePoints.includes("加工余量确定"), true);
});

test("rejects malformed diagnosis without leaking raw sensitive fields", () => {
  const extracted = extractLearningAssistantDiagnosis({
    diagnosis: {
      source: LEARNING_ASSISTANT_DIAGNOSIS_SOURCE,
      generatedAt: "",
      summary: "token C:\\private\\file should not be returned",
      pendingKnowledgePoints: ["工艺规程"],
      token: "secret",
      path: "C:\\secret\\db.sqlite",
    },
  });

  assert.equal(extracted, null);
});
