import { useMemo, useRef, useState } from "react";
import type { ChangeEvent, DragEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Form,
  Input,
  List,
  Radio,
  Space,
  Steps,
  Tag,
  Typography,
  message,
} from "antd";
import {
  BookOpen,
  CheckCircle2,
  FolderOpen,
  Lightbulb,
  ListChecks,
  PenLine,
  RotateCcw,
  SearchCheck,
  Target,
  Trash2,
  Upload,
} from "lucide-react";
import browserFallbackQuestions from "./browser_fallback_questions.json";
import browserFallbackResources from "./browser_fallback_resources.json";

const { Text, Title } = Typography;

const DEFAULT_ENGINE_ROOT = "../learning-assistant";
const PLACEHOLDER_MESSAGE =
  "该功能将在后续版本接入数据库/题库/进度记录。";

interface LearningAssistantFormValues {
  learningAssistantRoot: string;
  learningGoal: string;
  courseName: string;
  learningCycle: string;
  dailyTime: string;
  currentLevel: string;
  finalGoal: string;
}

interface LearningAssistantCheckResult {
  ok: boolean;
  skillPath: string;
  templatePath: string;
  errors: string[];
}

interface LearningAssistantUnderstanding {
  summary: string;
  currentGap: string;
  strategy: string;
  closedLoop: string;
}

interface LearningAssistantStage {
  name: string;
  timeRange: string;
  goal: string;
  learningTasks: string[];
  resourceTasks: string[];
  practiceTasks: string[];
  checkTasks: string[];
  completionCriteria: string[];
}

interface LearningAssistantPlanResult {
  success: boolean;
  engineRoot: string;
  skillPath: string;
  templatePath: string;
  understanding: LearningAssistantUnderstanding;
  stages: LearningAssistantStage[];
  error: string | null;
}

interface LearningResource {
  resourceId: string;
  title: string;
  type: string;
  course: string;
  knowledgePoint: string;
  difficulty: string;
  url: string;
  summary: string;
  tags: string[];
  duration: string;
  reason: string;
}

interface LearningResourcesRecommendResult {
  resources: LearningResource[];
  message: string;
  source: string;
}

interface StageResourceState {
  loading: boolean;
  resources: LearningResource[];
  message: string;
  error: string | null;
}

interface LearningQuizQuestion {
  questionId: string;
  course: string;
  knowledgePoint: string;
  type: "choice" | "judgment" | "short_answer" | string;
  question: string;
  options: string[];
  standardAnswer: string;
  keywords: string[];
  score: number;
  difficulty: string;
  explanation: string;
}

interface LearningQuizQuestionsResult {
  questions: LearningQuizQuestion[];
  message: string;
  source: string;
}

interface LearningQuizAnswer {
  questionId: string;
  userAnswer: string;
}

interface LearningQuizDetailResult {
  questionId: string;
  userAnswer: string;
  standardAnswer: string;
  score: number;
  maxScore: number;
  correct: boolean;
  missingKeywords: string[];
  comment: string;
}

interface LearningQuizScoreResult {
  totalScore: number;
  maxScore: number;
  level: string;
  weakPoints: string[];
  missingKeywords: string[];
  feedback: string;
  suggestions: string[];
  canGoNext: boolean;
  detailResults: LearningQuizDetailResult[];
}

interface StageQuizState {
  loading: boolean;
  scoring: boolean;
  questions: LearningQuizQuestion[];
  answers: Record<string, string>;
  message: string;
  error: string | null;
  scoreResult: LearningQuizScoreResult | null;
}

interface UploadedLearningMaterial {
  id: string;
  name: string;
  type: string;
  size: number;
  uploadedAt: string;
}

const BROWSER_FALLBACK_RESOURCES =
  browserFallbackResources.resources as LearningResource[];
const BROWSER_FALLBACK_QUESTIONS =
  browserFallbackQuestions.questions as LearningQuizQuestion[];
const MATERIAL_ACCEPT = ".pdf,.doc,.docx,.ppt,.pptx,.txt,.md,.xlsx,.xls,.csv";
const MATERIAL_ALLOWED_EXTENSIONS = new Set([
  "pdf",
  "doc",
  "docx",
  "ppt",
  "pptx",
  "txt",
  "md",
  "xlsx",
  "xls",
  "csv",
]);
const MATERIAL_MAX_SIZE = 50 * 1024 * 1024;

function buildCommandInput(values: LearningAssistantFormValues) {
  return {
    learningAssistantRoot: values.learningAssistantRoot.trim(),
    learningGoal: values.learningGoal.trim(),
    courseName: values.courseName.trim(),
    learningCycle: values.learningCycle.trim(),
    dailyTime: values.dailyTime.trim(),
    currentLevel: values.currentLevel.trim(),
    finalGoal: values.finalGoal.trim(),
  };
}

function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function buildMockResult(
  input: ReturnType<typeof buildCommandInput>,
  includeStages: boolean,
): LearningAssistantPlanResult {
  const course = input.courseName || "当前课程";
  const cycle = input.learningCycle || "一个学习周期";
  const dailyTime = input.dailyTime || "每天固定时间";
  const currentLevel = input.currentLevel || "基础待评估";
  const finalGoal = input.finalGoal || "完成可检验的学习成果";
  const engineRoot = input.learningAssistantRoot || DEFAULT_ENGINE_ROOT;

  return {
    success: true,
    engineRoot,
    skillPath: `${engineRoot}/skills/learning-assistant/SKILL.md`,
    templatePath: `${engineRoot}/templates/plan_template.json`,
    error: null,
    understanding: {
      summary: `你希望围绕「${course}」在 ${cycle} 内完成学习目标，每天投入 ${dailyTime}，最终达到「${finalGoal}」。`,
      currentGap: `当前基础为「${currentLevel}」。下一步需要补齐知识框架、稳定练习节奏，并设置阶段性检查标准。`,
      strategy:
        "建议采用“先搭框架、再学核心、随后专题练习、最后综合输出”的节奏推进。",
      closedLoop:
        "目标解析 -> 计划生成 -> 阶段任务 -> 资源推荐 -> 成果检查 -> 进度记录 -> 计划调整",
    },
    stages: includeStages
      ? [
          {
            name: "阶段 1：目标拆解与知识框架",
            timeRange: "第 1 阶段，建议占总周期 20%",
            goal: `建立「${course}」的知识地图，明确学习路径和检查方式。`,
            learningTasks: [
              "整理课程大纲，列出必须掌握的核心模块",
              "把学习目标拆成 5-8 个可检查的小目标",
              "安排固定学习时间，并建立每日记录表",
            ],
            resourceTasks: [
              "选择 1 套主教材或系统课程作为主线资料",
              "准备 1 个练习来源，用于后续阶段验证理解",
            ],
            practiceTasks: [
              "完成一次基础概念自测，记录不会的概念",
              "输出 1 页知识框架图或模块清单",
            ],
            checkTasks: [
              "检查是否能说清课程核心模块之间的关系",
              "标记 3 个最需要补强的薄弱点",
            ],
            completionCriteria: [
              "形成清晰的学习清单和时间安排",
              "能说明课程重点、难点和下一阶段任务",
            ],
          },
          {
            name: "阶段 2：核心知识学习",
            timeRange: "第 2 阶段，建议占总周期 30%",
            goal: "完成核心知识输入，建立可复述、可应用的理解。",
            learningTasks: [
              "按模块学习核心章节，每次学习后写出关键结论",
              "记录所有卡住的问题，并在当天或次日解决",
              "每周汇总一次重点概念和典型例题",
            ],
            resourceTasks: [
              "围绕难点补充讲义、案例或视频资料",
              "优先使用同一体系资料，避免频繁切换资料源",
            ],
            practiceTasks: [
              "完成每个模块后的基础练习",
              "把错题按概念不清、步骤错误、审题失误分类",
            ],
            checkTasks: [
              "闭卷复述每个模块的知识框架",
              "用 2-3 道例题验证核心概念是否理解",
            ],
            completionCriteria: [
              "核心模块均有笔记和练习记录",
              "高频概念可以独立解释并举例说明",
            ],
          },
          {
            name: "阶段 3：专题练习与能力巩固",
            timeRange: "第 3 阶段，建议占总周期 30%",
            goal: "通过集中练习把知识转化为稳定的应用能力。",
            learningTasks: [
              "按薄弱专题安排专项突破",
              "整理高频错误、易混概念和关键步骤",
              "每两天复盘一次练习结果，调整练习重点",
            ],
            resourceTasks: [
              "选择分层练习题或项目案例",
              "为薄弱点匹配补救资料和基础练习",
            ],
            practiceTasks: [
              "完成专题练习并记录正确率",
              "对难题进行二次讲解或重做",
            ],
            checkTasks: [
              "完成一次阶段测试或综合练习",
              "检查薄弱点是否减少，是否还有重复错误",
            ],
            completionCriteria: [
              "主要专题能在限定时间内完成",
              "错题原因能归类，并能用新练习验证修正效果",
            ],
          },
          {
            name: "阶段 4：综合输出与目标验收",
            timeRange: "最后阶段，建议占总周期 20%",
            goal: `围绕「${finalGoal}」完成最终验收。`,
            learningTasks: [
              "回顾全周期知识结构，补齐最后薄弱模块",
              "整理一份最终复习清单或成果说明",
              "模拟真实考核或真实项目流程",
            ],
            resourceTasks: [
              "保留最终复习资料和后续进阶资源入口",
              "筛选 1-2 个用于持续提高的资料来源",
            ],
            practiceTasks: [
              "完成综合模拟、作品或项目任务",
              "按真实标准限时演练并记录结果",
            ],
            checkTasks: [
              "对照最终目标逐项验收",
              "记录未达标项目，生成下一轮调整建议",
            ],
            completionCriteria: [
              "能独立完成综合任务",
              "输出结果达到预设目标，并有清晰的复盘记录",
            ],
          },
        ]
      : [],
  };
}

async function callLearningAssistant<T>(
  command: string,
  input: ReturnType<typeof buildCommandInput>,
): Promise<T> {
  if (isTauriRuntime()) {
    try {
      return await invoke<T>(command, { input });
    } catch (error) {
      message.warning(`后端调用失败，已切换为前端模拟生成：${String(error)}`);
    }
  }

  if (command === "learning_assistant_check") {
    return {
      ok: true,
      skillPath: `${input.learningAssistantRoot || DEFAULT_ENGINE_ROOT}/skills/learning-assistant/SKILL.md`,
      templatePath: `${input.learningAssistantRoot || DEFAULT_ENGINE_ROOT}/templates/plan_template.json`,
      errors: [],
    } as T;
  }

  return buildMockResult(input, command === "learning_assistant_generate_plan") as T;
}

function TaskList({ title, items }: { title: string; items: string[] }) {
  return (
    <div>
      <Text strong>{title}</Text>
      <List
        size="small"
        dataSource={items}
        renderItem={(item) => (
          <List.Item style={{ paddingLeft: 0, paddingRight: 0 }}>
            <Text>{item}</Text>
          </List.Item>
        )}
      />
    </div>
  );
}

function MaterialUploader() {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [dragging, setDragging] = useState(false);
  const [materials, setMaterials] = useState<UploadedLearningMaterial[]>([]);

  function handleFiles(files: FileList | File[]) {
    const nextMaterials: UploadedLearningMaterial[] = [];

    Array.from(files).forEach((file) => {
      const extension = getFileExtension(file.name);
      if (!MATERIAL_ALLOWED_EXTENSIONS.has(extension)) {
        message.error("当前文件格式暂不支持");
        return;
      }
      if (file.size > MATERIAL_MAX_SIZE) {
        message.error("文件过大，请上传 50MB 以内的学习资料");
        return;
      }

      // TODO: 后续桌面端可在这里调用 learning_materials_upload，
      // 将文件保存到个人资料库并进入解析/向量化流程。
      nextMaterials.push({
        id: `${file.name}-${file.size}-${file.lastModified}-${Date.now()}`,
        name: file.name,
        type: extension.toUpperCase(),
        size: file.size,
        uploadedAt: new Date().toLocaleString(),
      });
    });

    if (nextMaterials.length) {
      setMaterials((prev) => [...nextMaterials, ...prev]);
      message.success(`已添加 ${nextMaterials.length} 个学习资料`);
    }
  }

  function handleInputChange(event: ChangeEvent<HTMLInputElement>) {
    if (event.target.files) {
      handleFiles(event.target.files);
      event.target.value = "";
    }
  }

  function handleDrop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault();
    setDragging(false);
    if (event.dataTransfer.files.length) {
      handleFiles(event.dataTransfer.files);
    }
  }

  return (
    <Card
      title="个人学习资料上传"
      extra={
        materials.length ? (
          <Button size="small" danger onClick={() => setMaterials([])}>
            清空全部
          </Button>
        ) : null
      }
    >
      <input
        ref={inputRef}
        type="file"
        multiple
        accept={MATERIAL_ACCEPT}
        className="hidden"
        onChange={handleInputChange}
      />
      <div
        role="button"
        tabIndex={0}
        className={`flex min-h-36 cursor-pointer flex-col items-center justify-center rounded-md border-2 border-dashed px-4 py-6 text-center transition ${
          dragging
            ? "border-blue-500 bg-blue-50"
            : "border-slate-300 bg-slate-50 hover:border-blue-400"
        }`}
        onClick={() => inputRef.current?.click()}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            inputRef.current?.click();
          }
        }}
        onDragEnter={(event) => {
          event.preventDefault();
          setDragging(true);
        }}
        onDragOver={(event) => {
          event.preventDefault();
          event.dataTransfer.dropEffect = "copy";
          setDragging(true);
        }}
        onDragLeave={(event) => {
          event.preventDefault();
          setDragging(false);
        }}
        onDrop={handleDrop}
      >
        <Upload size={28} className="mb-2 text-blue-500" />
        <Text strong>拖拽学习资料到这里，或点击选择文件</Text>
        <Text type="secondary" className="mt-1">
          支持格式：PDF、DOCX、PPTX、TXT、MD、XLSX
        </Text>
        <Text type="secondary" className="mt-1 text-xs">
          单个文件不超过 50MB
        </Text>
      </div>

      <Alert
        className="mt-3"
        type="info"
        showIcon
        message="已上传的个人资料将在后续版本中用于补充学习资源推荐和阶段任务生成。"
      />

      {materials.length ? (
        <List
          className="mt-3"
          size="small"
          dataSource={materials}
          renderItem={(item) => (
            <List.Item
              actions={[
                <Button
                  key="delete"
                  type="text"
                  danger
                  size="small"
                  icon={<Trash2 size={14} />}
                  onClick={() =>
                    setMaterials((prev) => prev.filter((material) => material.id !== item.id))
                  }
                />,
              ]}
            >
              <List.Item.Meta
                title={<Text>{item.name}</Text>}
                description={
                  <Space wrap size="small">
                    <Tag>{item.type}</Tag>
                    <Text type="secondary">{formatFileSize(item.size)}</Text>
                    <Text type="secondary">{item.uploadedAt}</Text>
                  </Space>
                }
              />
            </List.Item>
          )}
        />
      ) : null}
    </Card>
  );
}

function getFileExtension(fileName: string) {
  const extension = fileName.split(".").pop();
  return extension ? extension.toLowerCase() : "";
}

function formatFileSize(size: number) {
  if (size >= 1024 * 1024) {
    return `${(size / 1024 / 1024).toFixed(2)} MB`;
  }
  return `${Math.max(size / 1024, 0.01).toFixed(2)} KB`;
}

function buildStageKnowledgePoints(stage: LearningAssistantStage) {
  return [
    stage.goal,
    ...stage.learningTasks,
    ...stage.resourceTasks,
    ...stage.practiceTasks,
  ].filter(Boolean);
}

function buildBrowserFallbackResources(params: {
  course: string;
  stage: LearningAssistantStage;
  stageIndex: number;
  level: string;
  limit: number;
}): LearningResource[] {
  const knowledgePoints = buildStageKnowledgePoints(params.stage).map((item) =>
    item.toLowerCase(),
  );
  const preferredDifficulty = getPreferredDifficulty(params.level);

  return BROWSER_FALLBACK_RESOURCES.filter(
    (resource) => resource.course === params.course,
  )
    .map((resource) => {
      const knowledgePoint = resource.knowledgePoint.toLowerCase();
      const title = resource.title.toLowerCase();
      const summary = resource.summary.toLowerCase();
      let score = 0;

      if (
        knowledgePoints.some(
          (point) =>
            point.includes(knowledgePoint) ||
            knowledgePoint.includes(point) ||
            title.includes(point) ||
            summary.includes(point),
        )
      ) {
        score += 40;
      }
      if (normalizeDifficulty(resource.difficulty) === preferredDifficulty) {
        score += 30;
      }
      if (
        resource.tags.some(
          (tag) =>
            tag.includes(`阶段${params.stageIndex}`) ||
            tag.includes(`阶段 ${params.stageIndex}`),
        )
      ) {
        score += 20;
      }

      return {
        score,
        resource: {
          ...resource,
          reason:
            resource.reason ||
            `浏览器 fallback 演示资源：该资源匹配当前阶段知识点「${resource.knowledgePoint}」，可用于 MVP 调试。`,
        },
      };
    })
    .sort((left, right) => right.score - left.score)
    .slice(0, Math.min(Math.max(params.limit, 1), 3))
    .map((item) => item.resource);
}

function getPreferredDifficulty(level: string) {
  if (/零基础|基础较差|较差|入门|beginner|weak/i.test(level)) return "easy";
  if (/较好|提高|进阶|hard|advanced/i.test(level)) return "hard";
  return "medium";
}

function normalizeDifficulty(difficulty: string) {
  if (/easy|入门|基础|简单/i.test(difficulty)) return "easy";
  if (/hard|提高|进阶|困难|综合/i.test(difficulty)) return "hard";
  return "medium";
}

function buildBrowserFallbackQuestions(params: {
  course: string;
  stage: LearningAssistantStage;
  level: string;
  limit: number;
}): LearningQuizQuestion[] {
  const knowledgePoints = buildStageKnowledgePoints(params.stage).map((item) =>
    item.toLowerCase(),
  );
  const preferredDifficulty = getPreferredDifficulty(params.level);

  return BROWSER_FALLBACK_QUESTIONS.filter(
    (question) => question.course === params.course,
  )
    .map((question) => {
      const knowledgePoint = question.knowledgePoint.toLowerCase();
      const text = question.question.toLowerCase();
      let score = 0;

      if (
        knowledgePoints.some(
          (point) =>
            point.includes(knowledgePoint) ||
            knowledgePoint.includes(point) ||
            text.includes(point),
        )
      ) {
        score += 40;
      }
      if (normalizeDifficulty(question.difficulty) === preferredDifficulty) {
        score += 20;
      }
      score += question.type === "choice" ? 12 : question.type === "judgment" ? 10 : 8;
      return { score, question };
    })
    .sort((left, right) => right.score - left.score)
    .slice(0, Math.min(Math.max(params.limit, 1), 10))
    .map((item) => item.question);
}

function scoreBrowserQuiz(
  questions: LearningQuizQuestion[],
  answers: LearningQuizAnswer[],
): LearningQuizScoreResult {
  const answerMap = new Map(answers.map((answer) => [answer.questionId, answer.userAnswer]));
  const detailResults = questions.map((question) =>
    scoreBrowserQuestion(question, answerMap.get(question.questionId) ?? ""),
  );
  const totalScore = detailResults.reduce((sum, item) => sum + item.score, 0);
  const maxScore = detailResults.reduce((sum, item) => sum + item.maxScore, 0);
  const percent = maxScore > 0 ? Math.floor((totalScore * 100) / maxScore) : 0;
  const weakPoints = uniqueValues(
    questions
      .filter((question) => {
        const detail = detailResults.find((item) => item.questionId === question.questionId);
        return detail ? detail.score < detail.maxScore : false;
      })
      .map((question) => question.knowledgePoint),
  );
  const missingKeywords = uniqueValues(
    detailResults.flatMap((detail) => detail.missingKeywords),
  );
  const { level, feedback } = getQuizLevel(percent);

  return {
    totalScore,
    maxScore,
    level,
    weakPoints,
    missingKeywords,
    feedback,
    suggestions: buildQuizSuggestions(percent, weakPoints),
    canGoNext: percent >= 70,
    detailResults,
  };
}

function scoreBrowserQuestion(
  question: LearningQuizQuestion,
  userAnswer: string,
): LearningQuizDetailResult {
  const normalizedUserAnswer = normalizeQuizAnswer(userAnswer);
  const normalizedStandardAnswer = normalizeQuizAnswer(question.standardAnswer);

  if (question.type === "choice" || question.type === "judgment") {
    const correct = normalizedUserAnswer === normalizedStandardAnswer;
    return {
      questionId: question.questionId,
      userAnswer,
      standardAnswer: question.standardAnswer,
      score: correct ? question.score : 0,
      maxScore: question.score,
      correct,
      missingKeywords: [],
      comment: correct ? "答案正确" : "答案与标准答案不一致",
    };
  }

  const answerLower = userAnswer.toLowerCase();
  const matchedCount = question.keywords.filter((keyword) =>
    answerLower.includes(keyword.toLowerCase()),
  ).length;
  const missingKeywords = question.keywords.filter(
    (keyword) => !answerLower.includes(keyword.toLowerCase()),
  );
  const score = question.keywords.length
    ? Math.round((question.score * matchedCount) / question.keywords.length)
    : normalizedUserAnswer === normalizedStandardAnswer
      ? question.score
      : 0;

  return {
    questionId: question.questionId,
    userAnswer,
    standardAnswer: question.standardAnswer,
    score,
    maxScore: question.score,
    correct: score === question.score,
    missingKeywords,
    comment: question.keywords.length
      ? `匹配到 ${matchedCount}/${question.keywords.length} 个关键词`
      : "未配置关键词，按标准答案完全匹配评分",
  };
}

function normalizeQuizAnswer(answer: string) {
  return answer.trim().toLowerCase().replace(/\s+/g, "");
}

function getQuizLevel(percent: number) {
  if (percent >= 85) {
    return { level: "掌握良好", feedback: "掌握良好，可以进入下一阶段。" };
  }
  if (percent >= 70) {
    return { level: "基本掌握", feedback: "基本掌握，可以进入下一阶段，但需要补强薄弱点。" };
  }
  if (percent >= 60) {
    return { level: "掌握不稳", feedback: "掌握不稳，建议减少新内容并补强本阶段知识点。" };
  }
  return { level: "建议重学", feedback: "建议重学本阶段，降低难度并再次测试。" };
}

function buildQuizSuggestions(percent: number, weakPoints: string[]) {
  const weakText = weakPoints.length ? weakPoints.join("、") : "本阶段核心知识点";
  if (percent >= 85) return ["保持当前节奏，可补充提高题或综合案例。"];
  if (percent >= 70) return [`进入下一阶段前复习：${weakText}。`];
  if (percent >= 60) return [`先补强：${weakText}。`, "减少新内容输入，增加基础练习和错题复盘。"];
  return [`重新学习：${weakText}。`, "完成基础概念复述后再进行阶段测试。"];
}

function uniqueValues(values: string[]) {
  return values.filter((value, index) => value && values.indexOf(value) === index);
}

function ResourceList({ state }: { state?: StageResourceState }) {
  if (!state || (!state.loading && !state.message && !state.error)) return null;

  if (state.error) {
    return <Alert type="error" showIcon message={state.error} />;
  }

  if (!state.loading && state.resources.length === 0) {
    return <Alert type="info" showIcon message={state.message || "当前数据库暂无匹配资源"} />;
  }

  return (
    <div className="space-y-3">
      {state.message ? <Alert type="info" showIcon message={state.message} /> : null}
      <List
        loading={state.loading}
        dataSource={state.resources}
        renderItem={(resource) => (
          <List.Item style={{ paddingLeft: 0, paddingRight: 0 }}>
            <Card size="small" className="w-full">
              <div className="space-y-2">
                <Space wrap>
                  <Text strong>{resource.title}</Text>
                  <Tag>{resource.type}</Tag>
                  <Tag color="blue">{resource.difficulty}</Tag>
                  <Tag color="green">{resource.knowledgePoint}</Tag>
                  <Tag>{resource.duration}</Tag>
                </Space>
                <Text>{resource.summary}</Text>
                <div>
                  <Text type="secondary">{resource.reason}</Text>
                </div>
                {resource.url ? (
                  <Button size="small" href={resource.url} target="_blank">
                    打开资源
                  </Button>
                ) : null}
              </div>
            </Card>
          </List.Item>
        )}
      />
    </div>
  );
}

function QuizPanel({
  state,
  onAnswerChange,
  onSubmit,
}: {
  state?: StageQuizState;
  onAnswerChange: (questionId: string, answer: string) => void;
  onSubmit: () => void;
}) {
  if (!state || (!state.loading && !state.message && !state.error && !state.questions.length)) {
    return null;
  }

  if (state.error) {
    return <Alert type="error" showIcon message={state.error} />;
  }

  if (!state.loading && state.questions.length === 0) {
    return <Alert type="info" showIcon message={state.message || "当前题库暂无匹配题目"} />;
  }

  return (
    <div className="space-y-3">
      {state.message ? <Alert type="info" showIcon message={state.message} /> : null}
      <List
        loading={state.loading}
        dataSource={state.questions}
        renderItem={(question, index) => (
          <List.Item style={{ paddingLeft: 0, paddingRight: 0 }}>
            <Card size="small" className="w-full">
              <div className="space-y-3">
                <Space wrap>
                  <Tag color="processing">第 {index + 1} 题</Tag>
                  <Tag>{formatQuestionType(question.type)}</Tag>
                  <Tag color="blue">{question.score} 分</Tag>
                  <Tag>{question.difficulty}</Tag>
                  <Tag color="green">{question.knowledgePoint}</Tag>
                </Space>
                <Text strong>{question.question}</Text>
                {question.type === "choice" || question.type === "judgment" ? (
                  <Radio.Group
                    value={state.answers[question.questionId]}
                    onChange={(event) => onAnswerChange(question.questionId, event.target.value)}
                  >
                    <Space direction="vertical">
                      {question.options.map((option) => (
                        <Radio key={option} value={option}>
                          {option}
                        </Radio>
                      ))}
                    </Space>
                  </Radio.Group>
                ) : (
                  <Input.TextArea
                    value={state.answers[question.questionId] ?? ""}
                    autoSize={{ minRows: 2, maxRows: 5 }}
                    placeholder="请输入你的答案"
                    onChange={(event) => onAnswerChange(question.questionId, event.target.value)}
                  />
                )}
              </div>
            </Card>
          </List.Item>
        )}
      />
      {state.questions.length ? (
        <Button type="primary" loading={state.scoring} onClick={onSubmit}>
          提交测试
        </Button>
      ) : null}
      {state.scoreResult ? <QuizScoreResultView result={state.scoreResult} /> : null}
    </div>
  );
}

function QuizScoreResultView({ result }: { result: LearningQuizScoreResult }) {
  return (
    <Card size="small" title="评分结果">
      <div className="space-y-3">
        <Space wrap>
          <Tag color="blue">
            总分：{result.totalScore} / {result.maxScore}
          </Tag>
          <Tag color={result.canGoNext ? "green" : "orange"}>{result.level}</Tag>
          <Tag color={result.canGoNext ? "green" : "red"}>
            {result.canGoNext ? "建议进入下一阶段" : "建议继续巩固本阶段"}
          </Tag>
        </Space>
        <Alert type={result.canGoNext ? "success" : "warning"} showIcon message={result.feedback} />
        <Descriptions column={1} size="small" bordered>
          <Descriptions.Item label="薄弱知识点">
            {result.weakPoints.length ? result.weakPoints.join("、") : "暂无明显薄弱点"}
          </Descriptions.Item>
          <Descriptions.Item label="缺失关键词">
            {result.missingKeywords.length ? result.missingKeywords.join("、") : "暂无"}
          </Descriptions.Item>
          <Descriptions.Item label="复习建议">
            {result.suggestions.length ? result.suggestions.join("；") : "保持当前节奏"}
          </Descriptions.Item>
        </Descriptions>
        <List
          size="small"
          dataSource={result.detailResults}
          renderItem={(detail) => (
            <List.Item style={{ paddingLeft: 0, paddingRight: 0 }}>
              <div className="w-full">
                <Space wrap>
                  <Text strong>{detail.questionId}</Text>
                  <Tag color={detail.correct ? "green" : "orange"}>
                    {detail.score} / {detail.maxScore} 分
                  </Tag>
                  <Text type="secondary">{detail.comment}</Text>
                </Space>
                <div className="mt-1">
                  <Text type="secondary">标准答案：{detail.standardAnswer}</Text>
                </div>
                {detail.missingKeywords.length ? (
                  <div>
                    <Text type="secondary">
                      缺失关键词：{detail.missingKeywords.join("、")}
                    </Text>
                  </div>
                ) : null}
              </div>
            </List.Item>
          )}
        />
      </div>
    </Card>
  );
}

function formatQuestionType(type: string) {
  if (type === "choice") return "选择题";
  if (type === "judgment") return "判断题";
  if (type === "short_answer") return "简答题";
  return type;
}

export default function LearningAssistantPage() {
  const [form] = Form.useForm<LearningAssistantFormValues>();
  const [checking, setChecking] = useState(false);
  const [understandingLoading, setUnderstandingLoading] = useState(false);
  const [planLoading, setPlanLoading] = useState(false);
  const [checkResult, setCheckResult] = useState<LearningAssistantCheckResult | null>(null);
  const [result, setResult] = useState<LearningAssistantPlanResult | null>(null);
  const [stageResources, setStageResources] = useState<Record<number, StageResourceState>>({});
  const [stageQuizzes, setStageQuizzes] = useState<Record<number, StageQuizState>>({});

  const currentStep = useMemo(() => {
    if (result?.stages.length) return 2;
    if (result?.understanding) return 1;
    return 0;
  }, [result]);

  async function pickEngineRoot() {
    if (!isTauriRuntime()) {
      message.info("浏览器调试模式下请直接填写 learning-assistant 目录路径");
      return;
    }

    const picked = await openDialog({
      directory: true,
      title: "选择 learning-assistant 根目录",
    });
    if (!picked || Array.isArray(picked)) return;
    form.setFieldValue("learningAssistantRoot", picked);
  }

  async function handleCheck() {
    try {
      const { learningAssistantRoot } = await form.validateFields(["learningAssistantRoot"]);
      setChecking(true);
      const checked = await callLearningAssistant<LearningAssistantCheckResult>("learning_assistant_check", {
        learningAssistantRoot: String(learningAssistantRoot ?? "").trim(),
        learningGoal: "",
        courseName: "",
        learningCycle: "",
        dailyTime: "",
        currentLevel: "",
        finalGoal: "",
      });
      setCheckResult(checked);
      message[checked.ok ? "success" : "warning"](
        checked.ok ? "AI 助学引擎检测通过" : "AI 助学引擎目录不完整",
      );
    } catch (error) {
      message.error(String(error));
    } finally {
      setChecking(false);
    }
  }

  async function handleUnderstand() {
    try {
      const values = await form.validateFields();
      setUnderstandingLoading(true);
      const understood = await callLearningAssistant<LearningAssistantPlanResult>(
        "learning_assistant_understand",
        buildCommandInput(values),
      );
      setResult(understood);
      message.success("目标理解已生成");
    } catch (error) {
      message.error(String(error));
    } finally {
      setUnderstandingLoading(false);
    }
  }

  async function handleGeneratePlan() {
    try {
      const values = await form.validateFields();
      setPlanLoading(true);
      const generated = await callLearningAssistant<LearningAssistantPlanResult>(
        "learning_assistant_generate_plan",
        buildCommandInput(values),
      );
      setResult(generated);
      message.success("学习计划已生成");
    } catch (error) {
      message.error(String(error));
    } finally {
      setPlanLoading(false);
    }
  }

  function showPlaceholder() {
    message.info(PLACEHOLDER_MESSAGE);
  }

  async function handleRecommendResources(stage: LearningAssistantStage, index: number) {
    const values = form.getFieldsValue();
    const course = String(values.courseName ?? "").trim();
    const level = String(values.currentLevel ?? "").trim();

    if (!course) {
      message.warning("请先填写课程名称");
      return;
    }

    if (!isTauriRuntime()) {
      const resources = buildBrowserFallbackResources({
        course,
        stage,
        stageIndex: index + 1,
        level,
        limit: 3,
      });
      setStageResources((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          resources,
          message: resources.length
            ? "浏览器 fallback 演示数据：正式环境将优先调用 learning_resources_recommend 获取数据库资源。"
            : "浏览器 fallback 演示数据中暂无匹配资源；正式环境请在桌面端调用数据库推荐接口。",
          error: null,
        },
      }));
      return;
    }

    setStageResources((prev) => ({
      ...prev,
      [index]: {
        loading: true,
        resources: prev[index]?.resources ?? [],
        message: "",
        error: null,
      },
    }));

    try {
      const recommended = await invoke<LearningResourcesRecommendResult>(
        "learning_resources_recommend",
        {
          input: {
            course,
            stageName: stage.name,
            stageIndex: index + 1,
            knowledgePoints: buildStageKnowledgePoints(stage),
            level,
            taskType: "resource",
            limit: 3,
          },
        },
      );

      setStageResources((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          resources: recommended.resources,
          message: recommended.message || "当前数据库暂无匹配资源",
          error: null,
        },
      }));
    } catch (error) {
      setStageResources((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          resources: [],
          message: "",
          error: `资源推荐失败：${String(error)}`,
        },
      }));
    }
  }

  async function handleStartQuiz(stage: LearningAssistantStage, index: number) {
    const values = form.getFieldsValue();
    const course = String(values.courseName ?? "").trim();
    const level = String(values.currentLevel ?? "").trim();

    if (!course) {
      message.warning("请先填写课程名称");
      return;
    }

    if (!isTauriRuntime()) {
      const questions = buildBrowserFallbackQuestions({
        course,
        stage,
        level,
        limit: 5,
      });
      setStageQuizzes((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          scoring: false,
          questions,
          answers: {},
          message: questions.length
            ? "浏览器 fallback 演示题库：正式环境将优先调用 learning_quiz_get_questions 获取数据库题目。"
            : "当前题库暂无匹配题目",
          error: null,
          scoreResult: null,
        },
      }));
      return;
    }

    setStageQuizzes((prev) => ({
      ...prev,
      [index]: {
        loading: true,
        scoring: false,
        questions: prev[index]?.questions ?? [],
        answers: prev[index]?.answers ?? {},
        message: "",
        error: null,
        scoreResult: null,
      },
    }));

    try {
      const quiz = await invoke<LearningQuizQuestionsResult>("learning_quiz_get_questions", {
        input: {
          course,
          stageName: stage.name,
          stageIndex: index + 1,
          knowledgePoints: buildStageKnowledgePoints(stage),
          level,
          limit: 5,
        },
      });
      setStageQuizzes((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          scoring: false,
          questions: quiz.questions,
          answers: {},
          message: quiz.message || "当前题库暂无匹配题目",
          error: null,
          scoreResult: null,
        },
      }));
    } catch (error) {
      setStageQuizzes((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          scoring: false,
          questions: [],
          answers: {},
          message: "",
          error: `获取测试题目失败：${String(error)}`,
          scoreResult: null,
        },
      }));
    }
  }

  function handleQuizAnswerChange(index: number, questionId: string, answer: string) {
    setStageQuizzes((prev) => ({
      ...prev,
      [index]: {
        ...prev[index],
        loading: prev[index]?.loading ?? false,
        scoring: prev[index]?.scoring ?? false,
        questions: prev[index]?.questions ?? [],
        message: prev[index]?.message ?? "",
        error: prev[index]?.error ?? null,
        scoreResult: prev[index]?.scoreResult ?? null,
        answers: {
          ...(prev[index]?.answers ?? {}),
          [questionId]: answer,
        },
      },
    }));
  }

  async function handleSubmitQuiz(index: number) {
    const quiz = stageQuizzes[index];
    if (!quiz?.questions.length) return;

    const answers = quiz.questions.map((question) => ({
      questionId: question.questionId,
      userAnswer: quiz.answers[question.questionId] ?? "",
    }));

    setStageQuizzes((prev) => ({
      ...prev,
      [index]: {
        ...quiz,
        scoring: true,
        error: null,
      },
    }));

    try {
      const scoreResult = isTauriRuntime()
        ? await invoke<LearningQuizScoreResult>("learning_quiz_score", {
            input: {
              stageName: result?.stages[index]?.name ?? `阶段 ${index + 1}`,
              stageIndex: index + 1,
              questions: quiz.questions,
              answers,
            },
          })
        : scoreBrowserQuiz(quiz.questions, answers);

      setStageQuizzes((prev) => ({
        ...prev,
        [index]: {
          ...quiz,
          answers: quiz.answers,
          scoring: false,
          scoreResult,
          error: null,
        },
      }));
    } catch (error) {
      setStageQuizzes((prev) => ({
        ...prev,
        [index]: {
          ...quiz,
          scoring: false,
          error: `提交测试失败：${String(error)}`,
        },
      }));
    }
  }

  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto max-w-6xl p-6">
        <div className="mb-4">
          <Title level={3} style={{ marginBottom: 4 }}>
            AI 助学
          </Title>
          <Text type="secondary">
            输入学习目标和基础信息，生成一次可执行的阶段学习计划。
          </Text>
        </div>

        <Card className="mb-4">
          <Steps
            current={currentStep}
            items={[
              { title: "目标输入" },
              { title: "目标解析" },
              { title: "阶段计划" },
            ]}
          />
        </Card>

        <div className="grid gap-4 lg:grid-cols-[380px_1fr]">
          <div className="space-y-4">
            <Card title="学习目标输入">
              <Form
                form={form}
                layout="vertical"
                initialValues={{
                  learningAssistantRoot: DEFAULT_ENGINE_ROOT,
                  learningGoal:
                    "4 周内系统掌握系统工程基础，并能完成一次课程综合复习。",
                  courseName: "系统工程",
                  learningCycle: "4 周",
                  dailyTime: "每天 1 小时",
                  currentLevel: "基础一般，掌握部分概念但缺少系统复习。",
                  finalGoal: "能梳理课程知识框架，并完成期末综合复习题。",
                }}
              >
                <Form.Item
                  name="learningAssistantRoot"
                  label="learning-assistant 根目录"
                  rules={[{ required: true, message: "请填写助学引擎根目录" }]}
                >
                  <Input
                    placeholder="例如 ../learning-assistant"
                    addonAfter={
                      <Button
                        type="text"
                        size="small"
                        icon={<FolderOpen size={14} />}
                        onClick={pickEngineRoot}
                      />
                    }
                  />
                </Form.Item>

                <Form.Item
                  name="learningGoal"
                  label="学习目标"
                  rules={[{ required: true, message: "请输入学习目标" }]}
                >
                  <Input.TextArea
                    autoSize={{ minRows: 4, maxRows: 8 }}
                    placeholder="例如：4 周内系统掌握机器学习基础，并能完成一个小型分类项目。"
                  />
                </Form.Item>

                <Form.Item
                  name="courseName"
                  label="课程名称"
                  rules={[{ required: true, message: "请输入课程名称" }]}
                >
                  <Input placeholder="例如：机器学习基础" />
                </Form.Item>

                <Form.Item
                  name="learningCycle"
                  label="学习周期"
                  rules={[{ required: true, message: "请输入学习周期" }]}
                >
                  <Input placeholder="例如：4 周 / 30 天 / 一个学期" />
                </Form.Item>

                <Form.Item
                  name="dailyTime"
                  label="每日学习时间"
                  rules={[{ required: true, message: "请输入每日学习时间" }]}
                >
                  <Input placeholder="例如：每天 1 小时" />
                </Form.Item>

                <Form.Item
                  name="currentLevel"
                  label="当前基础"
                  rules={[{ required: true, message: "请输入当前基础" }]}
                >
                  <Input.TextArea
                    autoSize={{ minRows: 2, maxRows: 4 }}
                    placeholder="例如：学过 Python，但没有系统学过机器学习。"
                  />
                </Form.Item>

                <Form.Item
                  name="finalGoal"
                  label="最终目标"
                  rules={[{ required: true, message: "请输入最终目标" }]}
                >
                  <Input.TextArea
                    autoSize={{ minRows: 2, maxRows: 4 }}
                    placeholder="例如：能独立完成课程作业，并讲清楚关键模型原理。"
                  />
                </Form.Item>

                <Space wrap>
                  <Button
                    icon={<SearchCheck size={15} />}
                    loading={checking}
                    onClick={handleCheck}
                  >
                    检测引擎
                  </Button>
                  <Button
                    icon={<Lightbulb size={15} />}
                    loading={understandingLoading}
                    onClick={handleUnderstand}
                  >
                    AI 理解目标
                  </Button>
                  <Button
                    type="primary"
                    icon={<ListChecks size={15} />}
                    loading={planLoading}
                    onClick={handleGeneratePlan}
                  >
                    生成学习计划
                  </Button>
                </Space>
              </Form>
            </Card>

            <MaterialUploader />

            {checkResult && (
              <Alert
                type={checkResult.ok ? "success" : "warning"}
                showIcon
                message={checkResult.ok ? "助学引擎可用" : "助学引擎目录不完整"}
                description={
                  <div className="space-y-1">
                    <div>Skill：{checkResult.skillPath}</div>
                    <div>Template：{checkResult.templatePath}</div>
                    {checkResult.errors.map((error) => (
                      <div key={error}>{error}</div>
                    ))}
                  </div>
                }
              />
            )}
          </div>

          <div className="space-y-4">
            <Card title="AI 理解结果">
              {result?.understanding ? (
                <Descriptions column={1} bordered size="small">
                  <Descriptions.Item label="目标摘要">
                    {result.understanding.summary}
                  </Descriptions.Item>
                  <Descriptions.Item label="当前差距">
                    {result.understanding.currentGap}
                  </Descriptions.Item>
                  <Descriptions.Item label="学习策略">
                    {result.understanding.strategy}
                  </Descriptions.Item>
                  <Descriptions.Item label="闭环思想">
                    {result.understanding.closedLoop}
                  </Descriptions.Item>
                </Descriptions>
              ) : (
                <Alert
                  type="info"
                  showIcon
                  message="等待目标解析"
                  description="填写左侧信息后点击“AI 理解目标”，这里会展示结构化理解结果。"
                />
              )}
            </Card>

            <Card
              title="学习计划结果"
              extra={
                result?.stages.length ? (
                  <Tag color="blue">{result.stages.length} 个阶段</Tag>
                ) : null
              }
            >
              {result?.stages.length ? (
                <div className="space-y-4">
                  {result.stages.map((stage, index) => (
                    <Card
                      key={stage.name}
                      size="small"
                      title={
                        <Space>
                          <Tag color="processing">阶段 {index + 1}</Tag>
                          <span>{stage.name}</span>
                        </Space>
                      }
                    >
                      <div className="space-y-4">
                        <Descriptions column={1} size="small" bordered>
                          <Descriptions.Item label="时间安排">
                            {stage.timeRange}
                          </Descriptions.Item>
                          <Descriptions.Item label="阶段目标">
                            {stage.goal}
                          </Descriptions.Item>
                        </Descriptions>

                        <div className="grid gap-4 md:grid-cols-2">
                          <TaskList title="学习任务" items={stage.learningTasks} />
                          <TaskList title="资源任务" items={stage.resourceTasks} />
                          <TaskList title="练习任务" items={stage.practiceTasks} />
                          <TaskList title="检验任务" items={stage.checkTasks} />
                        </div>

                        <TaskList title="完成标准" items={stage.completionCriteria} />

                        <Space wrap>
                          <Button
                            icon={<BookOpen size={15} />}
                            loading={stageResources[index]?.loading}
                            onClick={() => handleRecommendResources(stage, index)}
                          >
                            推荐资源
                          </Button>
                          <Button
                            icon={<PenLine size={15} />}
                            loading={stageQuizzes[index]?.loading}
                            onClick={() => handleStartQuiz(stage, index)}
                          >
                            开始测试
                          </Button>
                          <Button icon={<RotateCcw size={15} />} onClick={showPlaceholder}>
                            调整计划
                          </Button>
                        </Space>

                        <ResourceList state={stageResources[index]} />
                        <QuizPanel
                          state={stageQuizzes[index]}
                          onAnswerChange={(questionId, answer) =>
                            handleQuizAnswerChange(index, questionId, answer)
                          }
                          onSubmit={() => handleSubmitQuiz(index)}
                        />
                      </div>
                    </Card>
                  ))}
                </div>
              ) : (
                <Alert
                  type="info"
                  showIcon
                  message="等待计划生成"
                  description="点击“生成学习计划”后，将生成 3-5 个阶段，并展示每个阶段的学习、资源、练习、检验任务和完成标准。"
                />
              )}
            </Card>

            <Alert
              type="success"
              showIcon
              icon={<CheckCircle2 size={18} />}
              message="当前 MVP 已能完成一次学习计划生成"
              description="本版先实现目标解析、计划生成和阶段任务展示；资源推荐、测试、进度记录和计划调整会在后续接入数据库、题库和学习记录。"
            />

            <Alert
              type="info"
              showIcon
              icon={<Target size={18} />}
              message="调用方式"
              description={
                result
                  ? `当前使用的助学目录：${result.engineRoot}`
                  : "浏览器调试时使用前端模拟生成；Tauri 桌面环境会优先调用 learning-assistant 后端 command。"
              }
            />
          </div>
        </div>
      </div>
    </div>
  );
}
