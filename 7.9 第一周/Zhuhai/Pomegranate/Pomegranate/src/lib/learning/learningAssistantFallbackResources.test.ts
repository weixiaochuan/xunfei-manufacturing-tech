import assert from "node:assert/strict";
import test from "node:test";
import {
  findLearningAssistantFallbackQuestions,
  findLearningAssistantFallbackResources,
  getLearningAssistantFallbackQuestionBank,
  getLearningAssistantFallbackResourceBank,
} from "./learningAssistantFallbackResources.ts";

test("matches manufacturing fallback questions by knowledge point", () => {
  const questions = findLearningAssistantFallbackQuestions({
    courseName: "机械制造工艺学",
    knowledgePoints: ["工艺规程设计"],
    limit: 3,
  });

  assert.ok(questions.length > 0);
  assert.equal(questions[0].course, "机械制造工艺学");
  assert.equal(questions[0].knowledgePoint, "工艺规程设计");
  assert.equal(questions[0].sourceFile, "browser-fallback-questions");
});

test("matches fallback resources without local paths or fake urls", () => {
  const resources = findLearningAssistantFallbackResources({
    stageName: "定位基准与夹具",
    knowledgePoints: ["定位基准选择"],
    limit: 2,
  });

  assert.ok(resources.length > 0);
  assert.equal(resources[0].knowledgePoint, "定位基准选择");
  assert.equal("url" in resources[0], false);
  assert.equal("path" in resources[0], false);
  assert.equal("ownerUserId" in resources[0], false);
});

test("fallback banks are defensive copies", () => {
  const questions = getLearningAssistantFallbackQuestionBank();
  const resources = getLearningAssistantFallbackResourceBank();
  questions[0].keywords.push("mutated");
  resources[0].tags.push("mutated");

  assert.equal(getLearningAssistantFallbackQuestionBank()[0].keywords.includes("mutated"), false);
  assert.equal(getLearningAssistantFallbackResourceBank()[0].tags.includes("mutated"), false);
});
