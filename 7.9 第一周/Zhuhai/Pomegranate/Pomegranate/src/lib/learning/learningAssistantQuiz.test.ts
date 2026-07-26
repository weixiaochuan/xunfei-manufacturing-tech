import assert from "node:assert/strict";
import test from "node:test";
import {
  appendLearningAssistantQuizRecordToProgress,
  buildLearningAssistantQuizRecord,
  buildLearningAssistantStageQuiz,
  extractLearningAssistantQuizRecords,
  LEARNING_ASSISTANT_QUIZ_SOURCE,
  scoreLearningAssistantQuiz,
} from "./learningAssistantQuiz.ts";

const stage = {
  name: "阶段 1：工艺规程基础",
  goal: "理解工艺规程设计的基本流程",
  knowledgePoints: ["工艺规程设计", "定位基准选择"],
  learningTasks: ["绘制工艺路线草图"],
  practiceTasks: ["完成工序安排练习"],
  learningEntries: [
    {
      title: "工艺规程设计",
      section: "工艺过程",
      studyAction: "梳理工艺路线和工序安排",
      checkMethod: "能说明设计步骤",
      reason: "来自本地制造工艺学知识点",
      sourceFile: "机械制造工艺学-01.xlsx",
    },
    {
      title: "定位基准选择",
      section: "夹具与定位",
      studyAction: "比较粗基准和精基准",
      checkMethod: "能完成基准选择判断",
      reason: "来自本地制造工艺学知识点",
      sourceFile: "机械制造工艺学-02.xlsx",
    },
  ],
};

test("builds stage quiz from project plan without question bank data", () => {
  const questions = buildLearningAssistantStageQuiz({
    stage,
    stageIndex: 0,
    currentLevel: "基础一般：掌握部分概念，但缺少系统复习",
    limit: 4,
  });

  assert.equal(questions.length, 4);
  assert.deepEqual(
    questions.map((item) => item.type),
    ["choice", "judgment", "short_answer", "choice"],
  );
  assert.equal(questions[0].sourceFile, "机械制造工艺学-01.xlsx");
  assert.ok(questions[0].question.includes("工艺规程设计"));
  assert.ok(questions.every((item) => item.stageName === stage.name));
});

test("scores objective questions exactly and short answers by keywords", () => {
  const questions = buildLearningAssistantStageQuiz({ stage, stageIndex: 0, limit: 3 });
  const result = scoreLearningAssistantQuiz(questions, [
    { questionKey: questions[0].questionKey, userAnswer: questions[0].standardAnswer },
    { questionKey: questions[1].questionKey, userAnswer: "错误" },
    { questionKey: questions[2].questionKey, userAnswer: `${questions[2].knowledgePoint} 需要结合阶段目标练习` },
  ]);

  assert.equal(result.maxScore, 40);
  assert.equal(result.detailResults[0].correct, true);
  assert.equal(result.detailResults[1].score, 0);
  assert.ok(result.detailResults[2].score > 0);
  assert.ok(result.weakKnowledgePoints.includes("定位基准选择"));
  assert.ok(result.missingKeywords.length > 0);
});

test("builds and saves traceable quiz record in progress", () => {
  const questions = buildLearningAssistantStageQuiz({ stage, stageIndex: 1, limit: 2 });
  const answers = {
    [questions[0].questionKey]: questions[0].standardAnswer,
    [questions[1].questionKey]: "正确",
  };
  const scoreResult = scoreLearningAssistantQuiz(
    questions,
    questions.map((question) => ({
      questionKey: question.questionKey,
      userAnswer: answers[question.questionKey],
    })),
  );
  const record = buildLearningAssistantQuizRecord({
    stage,
    stageIndex: 1,
    questions,
    answers,
    scoreResult,
    testedAt: "2026-07-26T00:00:00.000Z",
  });
  const progress = appendLearningAssistantQuizRecordToProgress({ status: "planned" }, record);
  const extracted = extractLearningAssistantQuizRecords(progress);

  assert.equal(progress.status, "planned");
  assert.equal(progress.quizRecordCount, 1);
  assert.equal(extracted[0].source, LEARNING_ASSISTANT_QUIZ_SOURCE);
  assert.equal(extracted[0].stageIndex, 1);
  assert.equal(extracted[0].items[0].standardAnswer, questions[0].standardAnswer);
  assert.equal(extracted[0].items[0].explanation, questions[0].explanation);
});

test("preserves ordinary learning text containing token password and path words", () => {
  const customStage = {
    name: "阶段 2",
    learningEntries: [
      {
        title: "数控程序 path 规划",
        section: "token ring 不是认证令牌",
        studyAction: "说明 password 字段在课程案例中只是普通文本",
      },
    ],
  };
  const questions = buildLearningAssistantStageQuiz({ stage: customStage, stageIndex: 0 });
  const result = scoreLearningAssistantQuiz(questions, [
    { questionKey: questions[0].questionKey, userAnswer: questions[0].standardAnswer },
  ]);

  assert.ok(questions[0].question.includes("path"));
  assert.equal(result.totalScore, questions[0].score);
});

test("extracts only whitelisted quiz record fields", () => {
  const rawProgress = {
    quizRecords: [
      {
        recordKey: "record-1",
        source: LEARNING_ASSISTANT_QUIZ_SOURCE,
        testedAt: "2026-07-26T00:00:00.000Z",
        stageIndex: 0,
        stageName: "阶段 1",
        totalScore: 8,
        maxScore: 10,
        percentage: 80,
        level: "基本掌握",
        weakKnowledgePoints: ["工艺路线"],
        missingKeywords: ["定位"],
        feedback: "继续复习",
        suggestions: ["复习工艺路线"],
        canAdvance: true,
        ownerUserId: "should-not-leak",
        storageKey: "should-not-leak",
        path: "C:\\secret\\quiz.json",
        token: "fake-test-token",
        items: [
          {
            questionKey: "q1",
            question: "题目",
            questionType: "choice",
            options: ["A", "B"],
            userAnswer: "A",
            standardAnswer: "A",
            explanation: "解析",
            score: 10,
            maxScore: 10,
            correct: true,
            knowledgePoint: "工艺路线",
            missingKeywords: [],
            storageKey: "nested-should-not-leak",
          },
        ],
      },
      {
        recordKey: "bad",
        source: LEARNING_ASSISTANT_QUIZ_SOURCE,
        testedAt: "2026-07-26T00:00:00.000Z",
        stageIndex: -1,
      },
    ],
  };

  const extracted = extractLearningAssistantQuizRecords(rawProgress);
  assert.equal(extracted.length, 1);
  assert.equal("ownerUserId" in extracted[0], false);
  assert.equal("storageKey" in extracted[0], false);
  assert.equal("path" in extracted[0], false);
  assert.equal("storageKey" in extracted[0].items[0], false);
});
