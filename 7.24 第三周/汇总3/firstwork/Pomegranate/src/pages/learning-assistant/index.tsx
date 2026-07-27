import { useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent, DragEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { DataNode } from "antd/es/tree";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Collapse,
  Descriptions,
  Form,
  Input,
  InputNumber,
  List,
  Modal,
  Radio,
  Select,
  Space,
  Steps,
  Tag,
  Tree,
  Typography,
  message,
} from "antd";
import {
  BookOpen,
  CheckCircle2,
  FolderOpen,
  History,
  Lightbulb,
  ListChecks,
  Lock,
  PenLine,
  RotateCcw,
  Save,
  SearchCheck,
  Settings,
  Target,
  Trash2,
  Upload,
} from "lucide-react";
import browserFallbackQuestions from "./browser_fallback_questions.json";
import browserFallbackResources from "./browser_fallback_resources.json";
import DailyTimeWheelPicker from "./components/DailyTimeWheelPicker";
import {
  documentSourceApi,
  documentTreeApi,
  type DocumentTreeNode,
} from "@/lib/api";
import {
  runPluginPipelineAfterModel,
  runPluginPipelineBeforeModel,
  type PluginPipelineBeforeResult,
} from "@/services/pluginPipeline";

const { Text, Title } = Typography;

const DEFAULT_ENGINE_ROOT = "../learning-assistant";
const DEFAULT_ADJUSTMENT_THRESHOLDS: AdjustmentThresholds = {
  relearnThreshold: 60,
  excellentThreshold: 80,
};
const PLACEHOLDER_MESSAGE =
  "动态计划调整将在后续版本接入；当前已保留学习记录和调整记录结构。";
const FIXED_COURSE_NAME = "机械制造工艺学";
const CUSTOM_FINAL_GOAL = "自定义目标";
const GOAL_CYCLE_MAP: Record<string, string> = {
  "期末冲刺": "3天",
  "查漏补缺": "2周",
  "系统学习": "3周",
  "综合提升": "4周",
};
const LEARNING_GOAL_OPTIONS = Object.keys(GOAL_CYCLE_MAP);
type SourceImportanceLevel = "reference" | "normal" | "important" | "core";
interface SelectedLearningSource { documentSourceId: number; importanceLevel: SourceImportanceLevel }
const SOURCE_IMPORTANCE_OPTIONS: Array<{value:SourceImportanceLevel;label:string}> = [
  { value:"reference", label:"仅供参考（权重0，不自动加入计划）" },
  { value:"normal", label:"常规资料（权重1.0）" },
  { value:"important", label:"重点资料（权重1.5）" },
  { value:"core", label:"核心资料（权重2.0）" },
];

function flattenDocumentFiles(nodes: DocumentTreeNode[]): DocumentTreeNode[] {
  return nodes.flatMap((node) =>
    node.nodeType === "file" ? [node] : flattenDocumentFiles(node.children),
  );
}

function usableDocumentIds(node: DocumentTreeNode): number[] {
  if (node.nodeType === "file") {
    return node.canUseAsLearningSource && node.documentSourceId !== null
      ? [node.documentSourceId]
      : [];
  }
  return node.children.flatMap(usableDocumentIds);
}

function parseStatusLabel(status: DocumentTreeNode["parseStatus"]) {
  switch (status) {
    case "ready":
      return { text: "已就绪", color: "success" };
    case "parsing":
      return { text: "正在解析", color: "processing" };
    case "failed":
      return { text: "解析失败", color: "error" };
    case "unsupported":
      return { text: "不支持", color: "default" };
    default:
      return { text: "等待解析", color: "default" };
  }
}
const CURRENT_LEVEL_OPTIONS = [
  "零基础：基本没有学习过本课程",
  "基础较弱：学习过，但大部分知识点掌握不牢",
  "基础一般：掌握部分概念，但缺少系统复习",
  "基础较好：已经掌握主要内容，需要查漏补缺",
];
const FINAL_GOAL_OPTIONS = [
  "掌握课程基础概念",
  "梳理完整课程知识框架",
  "通过期末考试",
  "期末成绩达到70分以上",
  "期末成绩达到80分以上",
  "期末成绩达到90分以上",
  "能够完成课程综合题",
  "能够运用知识解决综合问题",
  CUSTOM_FINAL_GOAL,
];

interface LearningAssistantFormValues {
  learningAssistantRoot: string;
  learningGoal: string;
  courseName: string;
  learningCycle: string;
  dailyStudyHours: number;
  dailyTime?: string;
  currentLevel: string;
  finalGoal: string;
  finalGoalCustom?: string;
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
  source?: string;
}

interface LearningPlanEntry {
  entryId: string;
  title: string;
  section: string;
  entryType: string;
  masteryLevel: string;
  studyAction: string;
  practiceAction: string;
  checkMethod: string;
  expectedOutput: string;
  estimatedMinutes: number;
  reason: string;
  sourceFile?: string;
  sourceType: "knowledgeBase" | "modelFallback" | string;
  prerequisite?: string[];
  weakReason?: string;
  retryTask?: string;
}

interface LearningAssistantStage {
  name: string;
  timeRange: string;
  goal: string;
  knowledgePoints?: string[];
  learningEntries?: LearningPlanEntry[];
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
  planStrategy?: string;
  goalProfileSummary?: string;
  localAllocation?: LocalLearningPlanAllocation;
  message?: string;
  fallbackReason?: string;
  error: string | null;
}

interface LocalLearningPlanAllocation {
  timeSummary:{baselineCourseHours:number;targetHours:number;availableHours:number;plannedHours:number;missingHours:number;extraAvailableHours:number;targetCoverageRate:number;recommendedDailyHours:number;totalDays:number;dailyStudyHours:number};
  stageAllocations:Array<{stageKey:string;stageName:string;ratio:number;allocatedHours:number}>;
  sourceAllocations:Array<{documentSourceId:number;displayName:string;category:string;importanceLevel:SourceImportanceLevel;importanceWeight:number;allocatedHours:number;includedInPlan:boolean}>;
  dailyAllocations:Array<{dayIndex:number;plannedHours:number;remainingCapacityHours:number;stageAllocations:Array<{stageKey:string;hours:number}>}>;
  stageSourceAllocations?: Array<{stageKey:string;documentSourceId:number;allocatedHours:number}>;
  warnings:string[];
}

interface LearningAssistantAiConfigFormValues {
  apiBase: string;
  apiKey: string;
  model: string;
}

interface LearningAssistantAiConfigStatus {
  apiBase: string;
  model: string;
  hasApiKey: boolean;
  source: "user" | "runtime" | "env" | "fallback" | "notConfigured" | string;
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

interface LearningKbResultItem {
  documentId: number;
  sourceFile: string;
  sourceFolder: string;
  sourceType: string;
  fileType: string;
  weight: number;
  chunkIndex: number;
  sheetName: string;
  section: string;
  title: string;
  content: string;
  matchedKeywords: string[];
  score: number;
  reason: string;
}

interface LearningKbSearchResult {
  results: LearningKbResultItem[];
  message: string;
  warnings?: string[];
}

interface LocalKbSearchState {
  loading: boolean;
  results: LearningKbResultItem[];
  message: string;
  error: string | null;
}

interface StageKbState {
  loading: boolean;
  results: LearningKbResultItem[];
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
  questionImage?: string;
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
  answerImage?: string;
}

interface LearningQuizScoreResult {
  totalScore: number;
  maxScore: number;
  percentage?: number;
  level: string;
  weakPoints: string[];
  missingKeywords: string[];
  feedback: string;
  suggestions: string[];
  canGoNext: boolean;
  detailResults: LearningQuizDetailResult[];
  localAdjustmentPromptId?: string;
  adjustmentPromptShown?: boolean;
  localAdjustmentDecision?: "pending" | "accepted" | "declined";
  localAdjustmentDecidedAt?: string;
  localAdjustmentReason?: string;
  localAdjustmentSource?: "local_rule";
}

interface WrongQuestionReviewItem {
  questionId: string;
  questionText: string;
  questionType: string;
  options: string[];
  userAnswer: string;
  standardAnswer: string;
  answerImage?: string;
  score: number;
  maxScore: number;
  knowledgePoint: string;
  stageId: string;
  stageName: string;
  missingKeywords: string[];
  feedback: string;
  wrongReason: string;
  reviewSuggestion: string;
  createdAt: string;
}

interface WrongQuestionReviewPrompt {
  id: string;
  fromStageId: string;
  fromStageIndex: number;
  fromStageName: string;
  targetStageId: string;
  targetStageIndex: number;
  targetStageName: string;
  quizRecordId: string;
  masteryLevel: string;
  percentage: number;
  score: number;
  maxScore: number;
  wrongQuestions: WrongQuestionReviewItem[];
  weakPoints: string[];
  missingKeywords: string[];
  shown: boolean;
  dismissed: boolean;
  reviewed: boolean;
  reviewedQuestionIds: string[];
  userDecision?: "later" | "dismissed" | "reviewed";
  createdAt: string;
  updatedAt: string;
  lastShownAt?: string;
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

interface LearningProgressGoalSnapshot {
  course: string;
  learningGoal: string;
  learningCycle: string;
  dailyTime: string;
  currentLevel: string;
  finalGoal: string;
  learningAssistantRoot: string;
}

interface LearningStageStatusSnapshot {
  stageId: string;
  stageIndex: number;
  stageName: string;
  status: "not_started" | "in_progress" | "completed" | "needs_review";
  score?: number;
  maxScore?: number;
  weakPoints: string[];
  missingKeywords: string[];
  canAdvance?: boolean;
  updatedAt: string;
}

interface LearningTestRecordSnapshot {
  stageId: string;
  stageIndex: number;
  stageName: string;
  score: number;
  maxScore: number;
  percentage?: number;
  level: string;
  weakPoints: string[];
  missingKeywords: string[];
  wrongKnowledgePoints: string[];
  feedback: string;
  suggestions: string[];
  canAdvance: boolean;
  adjustmentPromptShown?: boolean;
  localAdjustmentDecision?: "pending" | "accepted" | "declined";
  localAdjustmentReason?: string;
  localAdjustmentSource?: "local_rule";
  testedAt: string;
}

interface LearningPlanAdjustmentSnapshot {
  beforePlan: LearningAssistantPlanResult | null;
  afterPlan: LearningAssistantPlanResult | null;
  reason: string;
  adjustedAt: string;
  source: string;
  needRetest: boolean;
}

interface LearningPlanAdjustResult {
  stages: LearningAssistantStage[];
  conclusion: string;
  reason: string;
  source: string;
  ruleBand: "excellent" | "basic" | "relearn" | string;
  currentStageStatus: string;
  canAdvance: boolean;
  needRetest: boolean;
  weakPoints: string[];
  addedTasks: string[];
  delayedTasks: string[];
  lockedStageIndexes: number[];
  beforeStages?: LearningAssistantStage[];
  adjustedAt?: string;
}

interface LocalAdjustmentPromptState {
  stageIndex: number;
  scoreResult: LearningQuizScoreResult;
  previewVisible: boolean;
}

interface AdjustmentThresholds {
  relearnThreshold: number;
  excellentThreshold: number;
}

interface LearningProgressRecord {
  version: string;
  projectId?: string;
  projectName?: string;
  courseName?: string;
  learningGoal?: string;
  learningCycle?: string;
  dailyTime?: string;
  currentLevel?: string;
  finalGoal?: string;
  adjustmentThresholds?: AdjustmentThresholds;
  goal: LearningProgressGoalSnapshot;
  plan: LearningAssistantPlanResult | null;
  currentStageIndex: number;
  stageStatuses: LearningStageStatusSnapshot[];
  stageResources: Record<number, StageResourceState>;
  stageKbStates: Record<number, StageKbState>;
  stageQuizzes: Record<number, StageQuizState>;
  planKbContext: LearningKbSearchResult | null;
  testRecords: LearningTestRecordSnapshot[];
  wrongQuestionReviewPrompts?: WrongQuestionReviewPrompt[];
  adjustments: LearningPlanAdjustmentSnapshot[];
  adjustResults?: Record<number, LearningPlanAdjustResult>;
  createdAt: string;
  updatedAt: string;
  lastOpenedAt?: string;
  planSource: string;
}

interface LearningProjectSummary {
  projectId: string;
  projectName: string;
  courseName: string;
  learningGoal: string;
  currentStage: string;
  progressPercent: number;
  createdAt: string;
  updatedAt: string;
  lastOpenedAt: string;
}

interface LearningProjectListResult {
  projects: LearningProjectSummary[];
  currentProjectId?: string | null;
  migratedLatest: boolean;
  message: string;
}

interface LearningProjectLoadResult {
  project: LearningProgressRecord | null;
  summary?: LearningProjectSummary | null;
  message: string;
  error?: string | null;
}

interface LearningProjectSaveResult {
  projectId: string;
  savedAt: string;
  summary: LearningProjectSummary;
  message: string;
}

interface LearningProjectDeleteResult {
  deleted: boolean;
  currentProjectId?: string | null;
  message: string;
}

interface LearningProgressClearResult {
  cleared: boolean;
  message: string;
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
const MODEL_CALL_FAILED_PREFIX = "\u6a21\u578b\u8c03\u7528\u5931\u8d25\uff1a";

function formatStudyHours(hours:number):string { const whole=Math.floor(hours);const minutes=Math.round((hours-whole)*60);if(!whole)return `每天${minutes}分钟`;return minutes?`每天${whole}小时${minutes}分钟`:`每天${whole}小时`; }
function parseStudyHours(value?: string): number { const text=String(value??"");const hours=Number(text.match(/([\d.]+)\s*小时/)?.[1]??0);const minutes=Number(text.match(/([\d.]+)\s*分钟/)?.[1]??0);return hours+minutes/60||1; }

function buildCommandInput(
  values: LearningAssistantFormValues,
  selectedLearningSources: SelectedLearningSource[] = [],
  pluginPromptContext?: string,
) {
  const finalGoal =
    values.finalGoal === CUSTOM_FINAL_GOAL
      ? String(values.finalGoalCustom ?? "").trim()
      : values.finalGoal.trim();

  return {
    learningAssistantRoot: values.learningAssistantRoot.trim(),
    learningGoal: values.learningGoal.trim(),
    courseName: FIXED_COURSE_NAME,
    learningCycle: values.learningCycle.trim(),
    dailyTime: formatStudyHours(values.dailyStudyHours),
    dailyStudyHours: values.dailyStudyHours,
    currentLevel: values.currentLevel.trim(),
    finalGoal,
    selectedDocumentSourceIds:selectedLearningSources.map((item)=>item.documentSourceId),
    selectedLearningSources,
    ...(pluginPromptContext ? { pluginPromptContext } : {}),
  };
}

function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function formatGenerationSource(source?: string) {
  const normalized = String(source ?? "").trim().toLowerCase();
  if (normalized === "user" || normalized === "runtime") {
    return "当前使用用户配置的模型生成";
  }
  if (normalized === "env") {
    return "当前使用环境变量模型生成";
  }
  if (normalized === "fallback" || normalized === "template" || !normalized) {
    return "当前使用本地模板生成";
  }
  if (normalized === "spark") {
    return "当前使用讯飞星火生成";
  }
  if (normalized === "fallback" || normalized === "template" || !normalized) {
    return "当前使用本地模板生成";
  }
  return `当前结果来源：${source}`;
}

function formatApiConfigSource(source?: string) {
  const normalized = String(source ?? "").trim().toLowerCase();
  if (normalized === "user" || normalized === "runtime") return "用户配置";
  if (normalized === "env") return "环境变量";
  return "本地模板";
}

function formatApiConfigStatus(status?: LearningAssistantAiConfigStatus | null) {
  if (!status?.hasApiKey) return "当前未配置模型 API，将使用本地模板";
  return `当前模型：${status.model}（来源：${formatApiConfigSource(status.source)}）`;
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
      source: "fallback",
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
              "标记 3 个最需要复习的薄弱点",
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
    // Desktop calls must stay on the real Rust path. Falling back here would
    // make a broken command or service look like a successful AI operation.
    return invoke<T>(command, { input });
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

function masteryColor(level?: string) {
  if (level === "熟练应用") return "purple";
  if (level === "掌握") return "orange";
  if (level === "理解") return "cyan";
  return "blue";
}

function LearningEntriesList({ entries }: { entries?: LearningPlanEntry[] }) {
  if (!entries?.length) return null;
  return (
    <div>
      <Text strong>本阶段具体词条</Text>
      <Collapse
        className="mt-2"
        size="small"
        items={entries.map((entry, index) => ({
          key: entry.entryId || `${entry.title}-${index}`,
          label: (
            <Space wrap size="small">
              <Text strong>{entry.title}</Text>
              <Tag color={masteryColor(entry.masteryLevel)}>{entry.masteryLevel || "理解"}</Tag>
              <Tag>{entry.estimatedMinutes || 20} 分钟</Tag>
              {entry.sourceType === "knowledgeBase" ? <Tag color="green">本地知识库</Tag> : <Tag>模板补充</Tag>}
            </Space>
          ),
          children: (
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label="所属章节">{entry.section || "未标注"}</Descriptions.Item>
              <Descriptions.Item label="词条类型">{entry.entryType || "概念"}</Descriptions.Item>
              <Descriptions.Item label="学习动作">{entry.studyAction}</Descriptions.Item>
              <Descriptions.Item label="练习动作">{entry.practiceAction}</Descriptions.Item>
              <Descriptions.Item label="检查方式">{entry.checkMethod}</Descriptions.Item>
              <Descriptions.Item label="预期成果">{entry.expectedOutput}</Descriptions.Item>
              <Descriptions.Item label="安排原因">{entry.reason}</Descriptions.Item>
              {entry.prerequisite?.length ? (
                <Descriptions.Item label="前置知识">
                  {entry.prerequisite.join("、")}
                </Descriptions.Item>
              ) : null}
              {entry.weakReason ? (
                <Descriptions.Item label="薄弱原因">{entry.weakReason}</Descriptions.Item>
              ) : null}
              {entry.retryTask ? (
                <Descriptions.Item label="再检查任务">{entry.retryTask}</Descriptions.Item>
              ) : null}
              <Descriptions.Item label="来源文件">
                {entry.sourceFile || "当前词条未匹配到本地知识库来源"}
              </Descriptions.Item>
            </Descriptions>
          ),
        }))}
      />
    </div>
  );
}

function MaterialUploader({ onImported }:{onImported:()=>Promise<void>}) {
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [dragging, setDragging] = useState(false);
  const [materials, setMaterials] = useState<UploadedLearningMaterial[]>([]);

  async function pickFiles() {
    if (!isTauriRuntime()) { inputRef.current?.click(); return; }
    const selected = await openDialog({ multiple:true, title:"导入助学资料到文档数据中心", filters:[{name:"学习资料",extensions:Array.from(MATERIAL_ALLOWED_EXTENSIONS)}] });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    let imported = 0;
    for (const sourcePath of paths) {
      try { await documentSourceApi.importLearning(sourcePath); imported += 1; }
      catch (error) { message.error(`导入失败：${String(error)}`); }
    }
    if (imported) { message.success(`已将 ${imported} 个文件登记到“助学模块上传”`); await onImported(); }
  }

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
        onClick={() => void pickFiles()}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            void pickFiles();
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
        message="桌面端文件会复制到统一文档数据目录，并登记到文档页面的“助学模块上传”。"
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
    stage.name,
    stage.goal,
    ...(stage.knowledgePoints ?? []),
    ...stage.learningTasks,
    ...stage.resourceTasks,
    ...stage.practiceTasks,
    ...stage.checkTasks,
  ].filter(Boolean);
}

function getWrongKnowledgePoints(quiz: StageQuizState) {
  const result = quiz.scoreResult;
  if (!result) return [];

  return Array.from(
    new Set(
      result.detailResults
        .filter((detail) => !detail.correct)
        .map((detail) => {
          const question = quiz.questions.find((item) => item.questionId === detail.questionId);
          return question?.knowledgePoint || detail.questionId;
        })
        .filter(Boolean),
    ),
  );
}

function isWrongQuizDetail(detail: LearningQuizDetailResult) {
  return detail.score < detail.maxScore || !detail.correct || detail.missingKeywords.length > 0;
}

function buildWrongReason(question: LearningQuizQuestion | undefined, detail: LearningQuizDetailResult) {
  if (!detail.correct && (question?.type === "choice" || question?.type === "judgment")) {
    return "答案与标准答案不一致";
  }
  if (detail.score < detail.maxScore && question?.type === "short_answer") {
    return detail.missingKeywords.length
      ? `简答题未拿满分，缺失关键词：${detail.missingKeywords.join("、")}`
      : "简答题未拿满分";
  }
  if (detail.score < detail.maxScore) {
    return "本题得分低于满分";
  }
  if (detail.missingKeywords.length) {
    return `缺失关键词：${detail.missingKeywords.join("、")}`;
  }
  return detail.comment || "评分结果显示需要复盘";
}

function buildWrongQuestionReviewItems(params: {
  quiz: StageQuizState;
  scoreResult: LearningQuizScoreResult;
  stage: LearningAssistantStage | undefined;
  stageIndex: number;
}) {
  const { quiz, scoreResult, stage, stageIndex } = params;
  const stageId = getStageId(stage, stageIndex);
  const stageName = stage?.name ?? `阶段 ${stageIndex + 1}`;
  const createdAt = nowText();

  return scoreResult.detailResults
    .filter(isWrongQuizDetail)
    .map((detail) => {
      const question = quiz.questions.find((item) => item.questionId === detail.questionId);
      const knowledgePoint = question?.knowledgePoint || detail.questionId;
      return {
        questionId: detail.questionId,
        questionText: question?.question ?? detail.questionId,
        questionType: question?.type ?? "unknown",
        options: question?.options ?? [],
        userAnswer: detail.userAnswer,
        standardAnswer: detail.standardAnswer || question?.standardAnswer || "",
        answerImage: detail.answerImage,
        score: detail.score,
        maxScore: detail.maxScore,
        knowledgePoint,
        stageId,
        stageName,
        missingKeywords: detail.missingKeywords ?? [],
        feedback: detail.comment || "建议复盘本题",
        wrongReason: buildWrongReason(question, detail),
        reviewSuggestion: `建议重新学习「${knowledgePoint}」相关内容，并完成同类基础题。`,
        createdAt,
      };
    });
}

function buildNextStageWrongQuestionReviewPrompt(params: {
  plan: LearningAssistantPlanResult | null;
  stageIndex: number;
  quiz: StageQuizState;
  scoreResult: LearningQuizScoreResult;
  thresholds: AdjustmentThresholds;
}) {
  const { plan, stageIndex, quiz, scoreResult, thresholds } = params;
  const percentage =
    scoreResult.percentage ?? getScorePercentage(scoreResult.totalScore, scoreResult.maxScore);
  const classified = classifyScoreByThresholds(percentage, thresholds);
  const targetStageIndex = stageIndex + 1;
  const fromStage = plan?.stages[stageIndex];
  const targetStage = plan?.stages[targetStageIndex];

  if (classified.band !== "basic" || !targetStage) return null;

  const wrongQuestions = buildWrongQuestionReviewItems({
    quiz,
    scoreResult,
    stage: fromStage,
    stageIndex,
  });
  if (!wrongQuestions.length) return null;

  const quizRecordId = buildScorePromptId(stageIndex, scoreResult, thresholds);
  const createdAt = nowText();
  return {
    id: `wrong-review-${quizRecordId}`,
    fromStageId: getStageId(fromStage, stageIndex),
    fromStageIndex: stageIndex,
    fromStageName: fromStage?.name ?? `阶段 ${stageIndex + 1}`,
    targetStageId: getStageId(targetStage, targetStageIndex),
    targetStageIndex,
    targetStageName: targetStage.name,
    quizRecordId,
    masteryLevel: "基本掌握",
    percentage,
    score: scoreResult.totalScore,
    maxScore: scoreResult.maxScore,
    wrongQuestions,
    weakPoints: uniqueValues([
      ...scoreResult.weakPoints,
      ...wrongQuestions.map((item) => item.knowledgePoint),
    ]),
    missingKeywords: uniqueValues([
      ...scoreResult.missingKeywords,
      ...wrongQuestions.flatMap((item) => item.missingKeywords),
    ]),
    shown: false,
    dismissed: false,
    reviewed: false,
    reviewedQuestionIds: [],
    createdAt,
    updatedAt: createdAt,
  } satisfies WrongQuestionReviewPrompt;
}

function syncWrongQuestionReviewPrompts(
  prompts: WrongQuestionReviewPrompt[],
  fromStageIndex: number,
  nextPrompt: WrongQuestionReviewPrompt | null,
) {
  const retained = prompts.filter((prompt) => {
    if (prompt.quizRecordId === nextPrompt?.quizRecordId) return false;
    if (prompt.fromStageIndex !== fromStageIndex) return true;
    return prompt.reviewed || prompt.dismissed;
  });
  return nextPrompt ? [...retained, nextPrompt] : retained;
}

function copyLearningStage(stage: LearningAssistantStage): LearningAssistantStage {
  return {
    ...stage,
    knowledgePoints: stage.knowledgePoints ? [...stage.knowledgePoints] : undefined,
    learningEntries: stage.learningEntries
      ? stage.learningEntries.map((entry) => ({
          ...entry,
          prerequisite: entry.prerequisite ? [...entry.prerequisite] : undefined,
        }))
      : undefined,
    learningTasks: [...stage.learningTasks],
    resourceTasks: [...stage.resourceTasks],
    practiceTasks: [...stage.practiceTasks],
    checkTasks: [...stage.checkTasks],
    completionCriteria: [...stage.completionCriteria],
  };
}

function buildScorePromptId(
  stageIndex: number,
  scoreResult: LearningQuizScoreResult,
  thresholds: AdjustmentThresholds,
) {
  const detailKey = scoreResult.detailResults
    .map((detail) => `${detail.questionId}:${detail.score}/${detail.maxScore}`)
    .join("|");
  return [
    stageIndex,
    scoreResult.totalScore,
    scoreResult.maxScore,
    scoreResult.percentage ?? getScorePercentage(scoreResult.totalScore, scoreResult.maxScore),
    thresholds.relearnThreshold,
    thresholds.excellentThreshold,
    detailKey,
  ].join("::");
}

function normalizeQuizScoreResult(
  scoreResult: LearningQuizScoreResult,
  stageIndex: number,
  thresholds: AdjustmentThresholds,
): LearningQuizScoreResult {
  const percentage =
    scoreResult.percentage ?? getScorePercentage(scoreResult.totalScore, scoreResult.maxScore);
  const { level, feedback } = getQuizLevel(percentage, thresholds);
  return {
    ...scoreResult,
    percentage,
    level,
    feedback,
    suggestions: scoreResult.suggestions?.length
      ? scoreResult.suggestions
      : buildQuizSuggestions(percentage, scoreResult.weakPoints, thresholds),
    canGoNext: percentage >= thresholds.relearnThreshold,
    localAdjustmentPromptId:
      scoreResult.localAdjustmentPromptId ||
      buildScorePromptId(
        stageIndex,
        {
          ...scoreResult,
          percentage,
        },
        thresholds,
      ),
  };
}

function shouldShowLowScorePrompt(
  scoreResult: LearningQuizScoreResult,
  thresholds: AdjustmentThresholds,
) {
  const percentage =
    scoreResult.percentage ?? getScorePercentage(scoreResult.totalScore, scoreResult.maxScore);
  return (
    percentage < thresholds.relearnThreshold &&
    !scoreResult.adjustmentPromptShown &&
    scoreResult.localAdjustmentDecision !== "accepted" &&
    scoreResult.localAdjustmentDecision !== "declined"
  );
}

function collectLocalAdjustmentWeakPoints(
  quiz: StageQuizState,
  scoreResult: LearningQuizScoreResult,
) {
  return uniqueValues([
    ...scoreResult.weakPoints,
    ...getWrongKnowledgePoints({ ...quiz, scoreResult }),
    ...scoreResult.missingKeywords.map((keyword) => `缺失关键词：${keyword}`),
  ]).slice(0, 8);
}

function buildLocalRelearnAdjustment(params: {
  plan: LearningAssistantPlanResult;
  stageIndex: number;
  quiz: StageQuizState;
  scoreResult: LearningQuizScoreResult;
  adjustedAt: string;
  thresholds: AdjustmentThresholds;
}): LearningPlanAdjustResult {
  const { plan, stageIndex, quiz, scoreResult, thresholds } = params;
  const stages = plan.stages.map(copyLearningStage);
  const currentStage = stages[stageIndex];
  const weakPoints = collectLocalAdjustmentWeakPoints(quiz, scoreResult);
  const weakText = weakPoints.length ? weakPoints.join("、") : "本阶段核心知识点";
  const learningTasks = [
    `重新学习薄弱知识点的基本概念：${weakText}。`,
    "重新梳理当前阶段核心内容，整理一页概念关系图或检查清单。",
    `查询本地知识库中的相关章节：${weakText}。`,
  ];
  const practiceTasks = [
    `增加基础选择题练习，优先覆盖：${weakText}。`,
    "增加判断题练习，逐题说明判断依据。",
    "增加基础简答题练习，用自己的话复述关键概念。",
    `优先练习本次错题涉及的知识点：${weakText}。`,
  ];
  const checkTasks = [
    "完成补学后重新测试。",
    `建议重新测试达到你设置的重学阈值 ${thresholds.relearnThreshold} 分以上，再继续推进后续学习。`,
  ];
  const addedTasks = [...learningTasks, ...practiceTasks, ...checkTasks];

  currentStage.learningTasks = uniqueValues([
    ...learningTasks,
    ...currentStage.learningTasks,
  ]).slice(0, 8);
  currentStage.resourceTasks = uniqueValues([
    `查询本地知识库中的相关章节：${weakText}。`,
    "复盘本阶段教材、课堂笔记和错题记录。",
    ...currentStage.resourceTasks,
  ]).slice(0, 6);
  currentStage.practiceTasks = uniqueValues([
    ...practiceTasks,
    ...currentStage.practiceTasks,
  ]).slice(0, 8);
  currentStage.checkTasks = uniqueValues([
    ...checkTasks,
    ...currentStage.checkTasks,
  ]).slice(0, 8);
  currentStage.completionCriteria = uniqueValues([
    "完成重新学习任务并能说明薄弱知识点的基本概念。",
    `重新测试达到你设置的重学阈值 ${thresholds.relearnThreshold} 分以上。`,
    ...currentStage.completionCriteria,
  ]).slice(0, 8);
  currentStage.goal = `${currentStage.goal}（重新学习：补齐薄弱知识点后再推进）`;

  return {
    stages,
    conclusion: "建议重新学习当前阶段，由你决定后续推进节奏。",
    reason: `本阶段测试低于你设置的重学阈值 ${thresholds.relearnThreshold} 分，系统根据薄弱知识点生成重新学习建议。`,
    source: "local_rule",
    ruleBand: "relearn",
    currentStageStatus: "重新学习",
    canAdvance: false,
    needRetest: true,
    weakPoints,
    addedTasks: uniqueValues(addedTasks).slice(0, 10),
    delayedTasks: [],
    lockedStageIndexes: [],
    adjustedAt: params.adjustedAt,
  };
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

function getScorePercentage(totalScore: number, maxScore: number) {
  if (!maxScore) return 0;
  return Math.min(100, Math.round((totalScore / maxScore) * 100));
}

function normalizeAdjustmentThresholds(value?: Partial<AdjustmentThresholds> | null) {
  const relearnThreshold = Number(value?.relearnThreshold);
  const excellentThreshold = Number(value?.excellentThreshold);
  if (
    Number.isFinite(relearnThreshold) &&
    Number.isFinite(excellentThreshold) &&
    relearnThreshold >= 0 &&
    excellentThreshold <= 100 &&
    relearnThreshold < excellentThreshold
  ) {
    return {
      relearnThreshold: Math.round(relearnThreshold),
      excellentThreshold: Math.round(excellentThreshold),
    };
  }
  return { ...DEFAULT_ADJUSTMENT_THRESHOLDS };
}

function validateAdjustmentThresholds(value: Partial<AdjustmentThresholds>) {
  const relearnThreshold = Number(value.relearnThreshold);
  const excellentThreshold = Number(value.excellentThreshold);
  return (
    Number.isFinite(relearnThreshold) &&
    Number.isFinite(excellentThreshold) &&
    relearnThreshold >= 0 &&
    excellentThreshold <= 100 &&
    relearnThreshold < excellentThreshold
  );
}

function classifyScoreByThresholds(percent: number, thresholds: AdjustmentThresholds) {
  if (percent >= thresholds.excellentThreshold) {
    return { level: "优秀", band: "excellent" as const };
  }
  if (percent >= thresholds.relearnThreshold) {
    return { level: "基本掌握", band: "basic" as const };
  }
  return { level: "需要重学", band: "relearn" as const };
}

function thresholdPreviewText(thresholds: AdjustmentThresholds) {
  return `优秀 ≥ ${thresholds.excellentThreshold}；基本掌握 ${thresholds.relearnThreshold}—${thresholds.excellentThreshold}；需要重学 < ${thresholds.relearnThreshold}`;
}

function scoreBrowserQuiz(
  questions: LearningQuizQuestion[],
  answers: LearningQuizAnswer[],
  thresholds = DEFAULT_ADJUSTMENT_THRESHOLDS,
): LearningQuizScoreResult {
  const answerMap = new Map(answers.map((answer) => [answer.questionId, answer.userAnswer]));
  const detailResults = questions.map((question) =>
    scoreBrowserQuestion(question, answerMap.get(question.questionId) ?? ""),
  );
  const totalScore = detailResults.reduce((sum, item) => sum + item.score, 0);
  const maxScore = detailResults.reduce((sum, item) => sum + item.maxScore, 0);
  const percent = getScorePercentage(totalScore, maxScore);
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
  const { level, feedback } = getQuizLevel(percent, thresholds);

  return {
    totalScore,
    maxScore,
    percentage: percent,
    level,
    weakPoints,
    missingKeywords,
    feedback,
    suggestions: buildQuizSuggestions(percent, weakPoints, thresholds),
    canGoNext: percent >= thresholds.relearnThreshold,
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

function getQuizLevel(percent: number, thresholds = DEFAULT_ADJUSTMENT_THRESHOLDS) {
  const classified = classifyScoreByThresholds(percent, thresholds);
  if (classified.band === "excellent") {
    return {
      level: "优秀",
      feedback: `达到你设置的优秀阈值 ${thresholds.excellentThreshold} 分，当前阶段掌握较好，可以正常进入后续阶段。`,
    };
  }
  if (classified.band === "basic") {
    return {
      level: "基本掌握",
      feedback: `达到你设置的重学阈值 ${thresholds.relearnThreshold} 分，可以继续后续学习，建议复习薄弱知识点。`,
    };
  }
  return {
    level: "需要重学",
    feedback: `本阶段测试结果低于你设置的重学阈值 ${thresholds.relearnThreshold} 分，建议重新学习当前阶段薄弱知识点。`,
  };
}

function buildQuizSuggestions(
  percent: number,
  weakPoints: string[],
  thresholds = DEFAULT_ADJUSTMENT_THRESHOLDS,
) {
  const weakText = weakPoints.length ? weakPoints.join("、") : "本阶段核心知识点";
  const classified = classifyScoreByThresholds(percent, thresholds);
  if (classified.band === "excellent") return ["保持当前节奏，可补充提高题或综合案例。"];
  if (classified.band === "basic") return [`继续后续学习前复习：${weakText}。`];
  return [
    `重新学习：${weakText}。`,
    `本阶段测试结果低于你设置的重学阈值 ${thresholds.relearnThreshold} 分，建议先完成薄弱点补学后再重新测试。`,
  ];
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

function LocalKbList({ state }: { state?: LocalKbSearchState }) {
  if (!state || (!state.loading && !state.message && !state.error)) return null;

  if (state.error) {
    return <Alert type="error" showIcon message={state.error} />;
  }

  if (!state.loading && state.results.length === 0) {
    return <Alert type="info" showIcon message={state.message || "当前本地知识库暂无匹配内容"} />;
  }

  return (
    <div className="space-y-3">
      {state.message ? <Alert type="info" showIcon message={state.message} /> : null}
      <List
        loading={state.loading}
        dataSource={state.results}
        renderItem={(item) => (
          <List.Item style={{ paddingLeft: 0, paddingRight: 0 }}>
            <Card size="small" className="w-full">
              <div className="space-y-2">
                <Space wrap>
                  <Text strong>{item.title}</Text>
                  <Tag color="purple">本地知识点</Tag>
                  <Tag>{item.section}</Tag>
                  <Tag>匹配分 {item.score}</Tag>
                </Space>
                <Text>{item.content}</Text>
                {item.matchedKeywords.length ? (
                  <Space wrap size="small">
                    {item.matchedKeywords.map((keyword) => (
                      <Tag key={keyword} color="blue">
                        {keyword}
                      </Tag>
                    ))}
                  </Space>
                ) : null}
                <div>
                  <Text type="secondary">
                    来源：{item.sourceFile} / {item.sheetName}
                  </Text>
                </div>
                <div>
                  <Text type="secondary">{item.reason}</Text>
                </div>
              </div>
            </Card>
          </List.Item>
        )}
      />
    </div>
  );
}

function StageKbList({ state }: { state?: StageKbState }) {
  if (!state || (!state.loading && !state.message && !state.error && !state.results.length)) {
    return null;
  }

  return <LocalKbList state={state} />;
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
                {question.questionImage ? (
                  <img
                    src={question.questionImage}
                    alt={`第 ${index + 1} 题题干图`}
                    className="max-h-[420px] max-w-full rounded-lg border border-gray-200 object-contain"
                  />
                ) : null}
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
          <Tag color="cyan">
            百分制：{result.percentage ?? getScorePercentage(result.totalScore, result.maxScore)} 分
          </Tag>
          <Tag color={result.canGoNext ? "green" : "orange"}>{result.level}</Tag>
          <Tag color={result.canGoNext ? "green" : "red"}>
            {result.canGoNext ? "可以继续后续学习" : "建议重新学习当前阶段"}
          </Tag>
          {result.localAdjustmentDecision === "accepted" ? (
            <Tag color="purple">已采用本地调整</Tag>
          ) : null}
          {result.localAdjustmentDecision === "declined" ? (
            <Tag color="default">已暂不调整</Tag>
          ) : null}
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
                {detail.answerImage ? (
                  <img
                    src={detail.answerImage}
                    alt={`${detail.questionId} 答案图`}
                    className="mt-2 max-h-[420px] max-w-full rounded-lg border border-gray-200 object-contain"
                  />
                ) : null}
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

function AdjustmentResultView({
  result,
  scoreResult,
  onUndo,
}: {
  result?: LearningPlanAdjustResult;
  scoreResult?: LearningQuizScoreResult | null;
  onUndo: () => void;
}) {
  if (!result) return null;

  const tags = buildAdjustmentTags(result);
  const before = result.beforeStages ?? [];
  const changedStages = result.stages
    .map((stage, index) => {
      const previous = before[index];
      if (!previous) return `阶段 ${index + 1}：新增或恢复`;
      const beforeCount =
        previous.learningTasks.length +
        previous.resourceTasks.length +
        previous.practiceTasks.length +
        previous.checkTasks.length;
      const afterCount =
        stage.learningTasks.length +
        stage.resourceTasks.length +
        stage.practiceTasks.length +
        stage.checkTasks.length;
      return beforeCount === afterCount && previous.goal === stage.goal
        ? null
        : `阶段 ${index + 1}：任务数 ${beforeCount} → ${afterCount}`;
    })
    .filter(Boolean) as string[];

  return (
    <Card size="small" title="计划调整结果">
      <div className="space-y-3">
        <Space wrap>
          {scoreResult ? (
            <Tag color="blue">
              本次测试：{scoreResult.totalScore}/{scoreResult.maxScore}
            </Tag>
          ) : null}
          <Tag color={result.source === "spark" ? "green" : "default"}>
            来源：{result.source === "spark" ? "讯飞星火" : "本地规则"}
          </Tag>
          {tags.map((tag) => (
            <Tag key={tag.text} color={tag.color}>
              {tag.text}
            </Tag>
          ))}
        </Space>

        <Alert
          type={result.canAdvance ? "success" : result.needRetest ? "warning" : "info"}
          showIcon
          message={result.conclusion}
          description={result.reason}
        />

        <Descriptions column={1} size="small" bordered>
          <Descriptions.Item label="薄弱知识点">
            {result.weakPoints.length ? result.weakPoints.join("、") : "暂无明显薄弱点"}
          </Descriptions.Item>
          <Descriptions.Item label="新增任务">
            {result.addedTasks.length ? result.addedTasks.join("；") : "无"}
          </Descriptions.Item>
          <Descriptions.Item label="减少或延后任务">
            {result.delayedTasks.length ? result.delayedTasks.join("；") : "无"}
          </Descriptions.Item>
          <Descriptions.Item label="是否需要重新测试">
            {result.needRetest ? "需要重新测试" : "暂不需要"}
          </Descriptions.Item>
          <Descriptions.Item label="调整前后差异">
            {changedStages.length ? changedStages.join("；") : "计划主体保持不变，仅更新阶段状态说明"}
          </Descriptions.Item>
        </Descriptions>

        {result.lockedStageIndexes.length ? (
          <Alert
            type="warning"
            showIcon
            message={`后续阶段建议优先稍后推进：${result.lockedStageIndexes
              .map((index) => `阶段 ${index + 1}`)
              .join("、")}`}
          />
        ) : null}

        {result.beforeStages?.length ? (
          <Button size="small" icon={<RotateCcw size={14} />} onClick={onUndo}>
            撤销上一次调整
          </Button>
        ) : null}
      </div>
    </Card>
  );
}

function buildAdjustmentTags(result: LearningPlanAdjustResult) {
  const tags = [{ text: "已调整", color: "blue" }];
  if (result.ruleBand === "excellent") {
    tags.push({ text: "提高任务", color: "purple" });
  }
  if (result.ruleBand === "basic") {
    tags.push({ text: "薄弱点复习", color: "orange" });
  }
  if (result.ruleBand === "relearn") {
    tags.push({ text: "重新学习", color: "red" });
  }
  if (result.lockedStageIndexes.length) {
    tags.push({ text: "建议先复习当前阶段", color: "orange" });
  }
  return tags;
}

function formatQuestionType(type: string) {
  if (type === "choice") return "选择题";
  if (type === "judgment") return "判断题";
  if (type === "short_answer") return "简答题";
  return type;
}

function nowText() {
  return new Date().toLocaleString();
}

function inferProjectNameFromValues(values: Partial<LearningAssistantFormValues>) {
  const course = String(values.courseName || FIXED_COURSE_NAME).trim();
  const goal = String(values.learningGoal || "").trim();
  return goal ? `${course}-${goal}` : `${course}学习项目`;
}

function getDefaultLearningValues(): LearningAssistantFormValues {
  return {
    learningAssistantRoot: DEFAULT_ENGINE_ROOT,
    courseName: FIXED_COURSE_NAME,
    learningGoal: "系统学习",
    learningCycle: GOAL_CYCLE_MAP["系统学习"],
    dailyStudyHours: 1,
    currentLevel: "基础一般：掌握部分概念，但缺少系统复习",
    finalGoal: "梳理完整课程知识框架",
  };
}

function getStageId(stage: LearningAssistantStage | undefined, index: number) {
  return `${index + 1}-${stage?.name || `阶段${index + 1}`}`;
}

function normalizeStageQuizzesForSave(stageQuizzes: Record<number, StageQuizState>) {
  return Object.fromEntries(
    Object.entries(stageQuizzes).map(([key, value]) => [
      key,
      {
        ...value,
        loading: false,
        scoring: false,
      },
    ]),
  ) as Record<number, StageQuizState>;
}

function buildStageStatuses(
  plan: LearningAssistantPlanResult | null,
  stageQuizzes: Record<number, StageQuizState>,
) {
  return (plan?.stages ?? []).map((stage, index) => {
    const scoreResult = stageQuizzes[index]?.scoreResult;
    const status: LearningStageStatusSnapshot["status"] = scoreResult
      ? scoreResult.canGoNext
        ? "completed"
        : "needs_review"
      : index === 0
        ? "in_progress"
        : "not_started";

    return {
      stageId: getStageId(stage, index),
      stageIndex: index,
      stageName: stage.name,
      status,
      score: scoreResult?.totalScore,
      maxScore: scoreResult?.maxScore,
      weakPoints: scoreResult?.weakPoints ?? [],
      missingKeywords: scoreResult?.missingKeywords ?? [],
      canAdvance: scoreResult?.canGoNext,
      updatedAt: nowText(),
    };
  });
}

function buildTestRecords(
  plan: LearningAssistantPlanResult | null,
  stageQuizzes: Record<number, StageQuizState>,
) {
  return Object.entries(stageQuizzes)
    .map(([indexText, quiz]) => {
      const index = Number(indexText);
      const scoreResult = quiz.scoreResult;
      const stage = plan?.stages[index];
      if (!scoreResult) return null;

      return {
        stageId: getStageId(stage, index),
        stageIndex: index,
        stageName: stage?.name ?? `阶段 ${index + 1}`,
        score: scoreResult.totalScore,
        maxScore: scoreResult.maxScore,
        percentage: scoreResult.percentage ?? getScorePercentage(scoreResult.totalScore, scoreResult.maxScore),
        level: scoreResult.level,
        weakPoints: scoreResult.weakPoints,
        missingKeywords: scoreResult.missingKeywords,
        wrongKnowledgePoints: scoreResult.detailResults
          .filter((detail) => !detail.correct)
          .map((detail) => {
            const question = quiz.questions.find((item) => item.questionId === detail.questionId);
            return question?.knowledgePoint || detail.questionId;
          })
          .filter(Boolean),
        feedback: scoreResult.feedback,
        suggestions: scoreResult.suggestions,
        canAdvance: scoreResult.canGoNext,
        adjustmentPromptShown: scoreResult.adjustmentPromptShown,
        localAdjustmentDecision: scoreResult.localAdjustmentDecision,
        localAdjustmentReason: scoreResult.localAdjustmentReason,
        localAdjustmentSource: scoreResult.localAdjustmentSource,
        testedAt: nowText(),
      };
    })
    .filter(Boolean) as LearningTestRecordSnapshot[];
}

function inferCurrentStageIndex(
  plan: LearningAssistantPlanResult | null,
  stageQuizzes: Record<number, StageQuizState>,
) {
  const stageCount = plan?.stages.length ?? 0;
  if (!stageCount) return 0;

  let current = 0;
  for (let index = 0; index < stageCount; index += 1) {
    const scoreResult = stageQuizzes[index]?.scoreResult;
    if (!scoreResult) {
      current = index;
      break;
    }
    current = scoreResult.canGoNext ? Math.min(index + 1, stageCount - 1) : index;
    if (!scoreResult.canGoNext) break;
  }
  return current;
}

export default function LearningAssistantPage() {
  const [form] = Form.useForm<LearningAssistantFormValues>();
  const [apiForm] = Form.useForm<LearningAssistantAiConfigFormValues>();
  const selectedFinalGoal = Form.useWatch("finalGoal", form);
  const selectedLearningCycle =
    Form.useWatch("learningCycle", form) || GOAL_CYCLE_MAP["系统学习"];
  const [checking, setChecking] = useState(false);
  const [understandingLoading, setUnderstandingLoading] = useState(false);
  const [planLoading, setPlanLoading] = useState(false);
  const [apiConfigOpen, setApiConfigOpen] = useState(false);
  const [apiConfigLoading, setApiConfigLoading] = useState(false);
  const [apiConfigSaving, setApiConfigSaving] = useState(false);
  const [apiConfigStatus, setApiConfigStatus] =
    useState<LearningAssistantAiConfigStatus | null>(null);
  const [pluginEnhancementStatus, setPluginEnhancementStatus] = useState<{
    executed: string[];
    warnings: string[];
  } | null>(null);
  const [checkResult, setCheckResult] = useState<LearningAssistantCheckResult | null>(null);
  const [result, setResult] = useState<LearningAssistantPlanResult | null>(null);
  const [stageResources, setStageResources] = useState<Record<number, StageResourceState>>({});
  const [localKbQuery, setLocalKbQuery] = useState("");
  const [localKbState, setLocalKbState] = useState<LocalKbSearchState>({
    loading: false,
    results: [],
    message: "",
    error: null,
  });
  const [planKbContext, setPlanKbContext] = useState<LearningKbSearchResult | null>(null);
  const [stageKbStates, setStageKbStates] = useState<Record<number, StageKbState>>({});
  const [stageQuizzes, setStageQuizzes] = useState<Record<number, StageQuizState>>({});
  const [wrongQuestionReviewPrompts, setWrongQuestionReviewPrompts] = useState<
    WrongQuestionReviewPrompt[]
  >([]);
  const [activeWrongQuestionReviewPromptId, setActiveWrongQuestionReviewPromptId] =
    useState<string | null>(null);
  const [wrongQuestionReviewExpanded, setWrongQuestionReviewExpanded] = useState(false);
  const [adjustingStages, setAdjustingStages] = useState<Record<number, boolean>>({});
  const [adjustResults, setAdjustResults] = useState<Record<number, LearningPlanAdjustResult>>({});
  const [localAdjustmentPrompt, setLocalAdjustmentPrompt] =
    useState<LocalAdjustmentPromptState | null>(null);
  const [adjustmentThresholds, setAdjustmentThresholds] = useState<AdjustmentThresholds>({
    ...DEFAULT_ADJUSTMENT_THRESHOLDS,
  });
  const [thresholdDraft, setThresholdDraft] = useState<AdjustmentThresholds>({
    ...DEFAULT_ADJUSTMENT_THRESHOLDS,
  });
  const [progressModalOpen, setProgressModalOpen] = useState(false);
  const [progressLoading, setProgressLoading] = useState(false);
  const [progressSaving, setProgressSaving] = useState(false);
  const [progressRecord, setProgressRecord] = useState<LearningProgressRecord | null>(null);
  const [lastSavedAt, setLastSavedAt] = useState("");
  const [progressSaveError, setProgressSaveError] = useState<string | null>(null);
  const [projectList, setProjectList] = useState<LearningProjectSummary[]>([]);
  const [currentProjectId, setCurrentProjectId] = useState<string | null>(null);
  const [currentProjectName, setCurrentProjectName] = useState("");
  const [projectManagerOpen, setProjectManagerOpen] = useState(false);
  const [projectDirty, setProjectDirty] = useState(false);
  const [unsavedConfirmOpen, setUnsavedConfirmOpen] = useState(false);
  const unsavedResolverRef = useRef<((value: boolean) => void) | null>(null);
  const projectDirtyRef = useRef(false);
  const wrongQuestionPromptSessionRef = useRef<Set<string>>(new Set());
  const [documentTree, setDocumentTree] = useState<DocumentTreeNode[]>([]);
  const [documentTreeLoading, setDocumentTreeLoading] = useState(false);
  const [selectedDocumentSourceIds, setSelectedDocumentSourceIds] = useState<number[]>([]);
  const [sourceImportanceLevels, setSourceImportanceLevels] = useState<Record<number, SourceImportanceLevel>>({});

  async function loadDocumentSources(forceRefresh = false) {
    if (!isTauriRuntime()) {
      setDocumentTree([]);
      return;
    }
    setDocumentTreeLoading(true);
    try {
      const listed = await documentTreeApi.list(forceRefresh);
      setDocumentTree(listed.roots);
      const files = flattenDocumentFiles(listed.roots);
      const usableIds = new Set(
        files
          .filter(
            (source) =>
              source.canUseAsLearningSource &&
              source.documentSourceId !== null,
          )
          .map((source) => source.documentSourceId as number),
      );
      setSelectedDocumentSourceIds((previous) => {
        const valid = previous.filter((id) => usableIds.has(id));
        if (previous.length) return valid;
        return files
          .filter(
            (source) =>
              source.sourceType === "localKnowledgeBase" &&
              source.documentSourceId !== null &&
              usableIds.has(source.documentSourceId),
          )
          .map((source) => source.documentSourceId as number);
      });
      if (listed.warnings.length) message.warning(listed.warnings.join("；"));
    } catch (error) {
      message.warning(`读取文档数据源失败：${String(error)}`);
    } finally {
      setDocumentTreeLoading(false);
    }
  }

  const selectedLearningSources = useMemo<SelectedLearningSource[]>(
    () => selectedDocumentSourceIds.map((documentSourceId) => ({
      documentSourceId,
      importanceLevel: sourceImportanceLevels[documentSourceId] ?? "normal",
    })),
    [selectedDocumentSourceIds, sourceImportanceLevels],
  );

  const documentFiles = useMemo(
    () => flattenDocumentFiles(documentTree),
    [documentTree],
  );

  const selectedReferenceFiles = useMemo(() => {
    const selectedSet = new Set(selectedDocumentSourceIds);
    const files: Array<{ source: DocumentTreeNode; folderName: string }> = [];
    const visit = (nodes: DocumentTreeNode[], folderName = "根目录") => {
      for (const node of nodes) {
        if (node.nodeType === "folder") {
          visit(node.children, node.name);
        } else if (
          node.documentSourceId !== null &&
          selectedSet.has(node.documentSourceId)
        ) {
          files.push({ source: node, folderName });
        }
      }
    };
    visit(documentTree);
    return files;
  }, [documentTree, selectedDocumentSourceIds]);

  const actualReferenceFiles = useMemo(() => {
    const grouped = new Map<
      number,
      {
        documentId: number;
        sourceFile: string;
        sourceFolder: string;
        fileType: string;
        weight: number;
        chunkCount: number;
      }
    >();
    for (const item of planKbContext?.results ?? []) {
      const existing = grouped.get(item.documentId);
      if (existing) {
        existing.chunkCount += 1;
      } else {
        grouped.set(item.documentId, {
          documentId: item.documentId,
          sourceFile: item.sourceFile,
          sourceFolder: item.sourceFolder,
          fileType: item.fileType,
          weight: item.weight,
          chunkCount: 1,
        });
      }
    }
    return [...grouped.values()];
  }, [planKbContext]);

  const learningSourceTreeData = useMemo(() => {
    const selectedSet = new Set(selectedDocumentSourceIds);
    const mapNode = (
      node: DocumentTreeNode,
      currentFolder: string,
    ): DataNode => {
      if (node.nodeType === "folder") {
        const availableIds = usableDocumentIds(node);
        const selectedCount = availableIds.filter((id) => selectedSet.has(id)).length;
        const checked = availableIds.length > 0 && selectedCount === availableIds.length;
        const indeterminate = selectedCount > 0 && !checked;
        return {
          key: node.id,
          title: (
            <div className="flex items-center gap-2 min-w-0">
              <Checkbox
                checked={checked}
                indeterminate={indeterminate}
                disabled={availableIds.length === 0}
                onClick={(event) => event.stopPropagation()}
                onChange={(event) => {
                  setSelectedDocumentSourceIds((previous) => {
                    const next = new Set(previous);
                    for (const id of availableIds) {
                      if (event.target.checked) next.add(id);
                      else next.delete(id);
                    }
                    return [...next];
                  });
                }}
              />
              <FolderOpen size={15} />
              <span className="truncate" title={node.name}>{node.name}</span>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {node.childCount}
              </Text>
            </div>
          ),
          children: node.children.map((child) => mapNode(child, node.name)),
        };
      }

      const documentId = node.documentSourceId as number;
      const selected = selectedSet.has(documentId);
      const status = parseStatusLabel(node.parseStatus);
      return {
        key: node.id,
        isLeaf: true,
        title: (
          <div
            className="flex items-center gap-2 min-w-0"
            style={{ padding: "3px 0", width: "100%" }}
            title={node.parseMessage || node.name}
          >
            <Checkbox
              checked={selected}
              disabled={!node.canUseAsLearningSource}
              onClick={(event) => event.stopPropagation()}
              onChange={(event) =>
                setSelectedDocumentSourceIds((previous) =>
                  event.target.checked
                    ? [...new Set([...previous, documentId])]
                    : previous.filter((id) => id !== documentId),
                )
              }
            />
            <span
              className="truncate"
              style={{ flex: "1 1 220px", minWidth: 120 }}
            >
              {node.name}
            </span>
            <Tag>{(node.fileType || "md").toUpperCase()}</Tag>
            <Text type="secondary" style={{ fontSize: 12, minWidth: 72 }}>
              {currentFolder}
            </Text>
            <Tag color={status.color}>{status.text}</Tag>
            <Select
              size="small"
              aria-label={`${node.name} 权重`}
              value={sourceImportanceLevels[documentId] ?? "normal"}
              options={SOURCE_IMPORTANCE_OPTIONS}
              disabled={!selected}
              style={{ width: 178, flexShrink: 0 }}
              onClick={(event) => event.stopPropagation()}
              onChange={(value) =>
                setSourceImportanceLevels((levels) => ({
                  ...levels,
                  [documentId]: value,
                }))
              }
            />
          </div>
        ),
      };
    };
    return documentTree.map((node) => mapNode(node, node.name));
  }, [documentTree, selectedDocumentSourceIds, sourceImportanceLevels]);

  const currentStep = useMemo(() => {
    if (result?.stages.length) return 2;
    if (result?.understanding) return 1;
    return 0;
  }, [result]);

  const inferredCurrentStageIndex = useMemo(
    () => inferCurrentStageIndex(result, stageQuizzes),
    [result, stageQuizzes],
  );

  const activeWrongQuestionReviewPrompt = useMemo(
    () =>
      wrongQuestionReviewPrompts.find(
        (prompt) => prompt.id === activeWrongQuestionReviewPromptId,
      ) ?? null,
    [activeWrongQuestionReviewPromptId, wrongQuestionReviewPrompts],
  );

  function buildProgressRecord(overrides?: {
    plan?: LearningAssistantPlanResult | null;
    stageQuizzes?: Record<number, StageQuizState>;
    stageResources?: Record<number, StageResourceState>;
    stageKbStates?: Record<number, StageKbState>;
    planKbContext?: LearningKbSearchResult | null;
    adjustResults?: Record<number, LearningPlanAdjustResult>;
    adjustments?: LearningPlanAdjustmentSnapshot[];
    wrongQuestionReviewPrompts?: WrongQuestionReviewPrompt[];
    createdAt?: string;
    adjustmentThresholds?: AdjustmentThresholds;
  }): LearningProgressRecord {
    const values = form.getFieldsValue();
    const finalGoal =
      values.finalGoal === CUSTOM_FINAL_GOAL
        ? String(values.finalGoalCustom ?? "").trim()
        : String(values.finalGoal ?? "").trim();
    const plan = overrides?.plan ?? result;
    const quizzes = normalizeStageQuizzesForSave(overrides?.stageQuizzes ?? stageQuizzes);
    const resources = overrides?.stageResources ?? stageResources;
    const kbStates = overrides?.stageKbStates ?? stageKbStates;
    const savedPlanKbContext = overrides?.planKbContext ?? planKbContext;
    const savedAdjustResults = overrides?.adjustResults ?? adjustResults;
    const savedWrongQuestionReviewPrompts =
      overrides?.wrongQuestionReviewPrompts ?? wrongQuestionReviewPrompts;
    const updatedAt = nowText();

    return {
      version: "1",
      projectId: currentProjectId ?? undefined,
      projectName:
        currentProjectName || inferProjectNameFromValues(values),
      courseName: String(values.courseName ?? FIXED_COURSE_NAME),
      learningGoal: String(values.learningGoal ?? ""),
      learningCycle: String(values.learningCycle ?? ""),
      dailyTime: formatStudyHours(values.dailyStudyHours ?? parseStudyHours(values.dailyTime)),
      currentLevel: String(values.currentLevel ?? ""),
      finalGoal,
      adjustmentThresholds: normalizeAdjustmentThresholds(
        overrides?.adjustmentThresholds ?? adjustmentThresholds,
      ),
      goal: {
        course: String(values.courseName ?? FIXED_COURSE_NAME),
        learningGoal: String(values.learningGoal ?? ""),
        learningCycle: String(values.learningCycle ?? ""),
        dailyTime: formatStudyHours(values.dailyStudyHours ?? parseStudyHours(values.dailyTime)),
        currentLevel: String(values.currentLevel ?? ""),
        finalGoal,
        learningAssistantRoot: String(values.learningAssistantRoot ?? DEFAULT_ENGINE_ROOT),
      },
      plan,
      currentStageIndex: inferCurrentStageIndex(plan, quizzes),
      stageStatuses: buildStageStatuses(plan, quizzes),
      stageResources: resources,
      stageKbStates: kbStates,
      stageQuizzes: quizzes,
      planKbContext: savedPlanKbContext,
      testRecords: buildTestRecords(plan, quizzes),
      wrongQuestionReviewPrompts: savedWrongQuestionReviewPrompts,
      adjustments: overrides?.adjustments ?? progressRecord?.adjustments ?? [],
      adjustResults: savedAdjustResults,
      createdAt: overrides?.createdAt ?? progressRecord?.createdAt ?? updatedAt,
      updatedAt,
      lastOpenedAt: progressRecord?.lastOpenedAt ?? updatedAt,
      planSource: plan?.understanding?.source ?? "unknown",
    };
  }

  async function saveProgressRecord(
    record: LearningProgressRecord,
    options?: { throwOnError?: boolean; silent?: boolean },
  ) {
    setProgressSaving(true);
    setProgressSaveError(null);
    try {
      if (!isTauriRuntime()) {
        setProgressRecord(record);
        setLastSavedAt(record.updatedAt);
        setProjectDirty(false);
        return;
      }

      let saved: LearningProjectSaveResult;
      if (currentProjectId) {
        saved = await invoke<LearningProjectSaveResult>("learning_project_save", {
          input: { projectId: currentProjectId, record },
        });
      } else {
        saved = await invoke<LearningProjectSaveResult>("learning_project_create", {
          input: {
            projectName: record.projectName || inferProjectNameFromValues(form.getFieldsValue()),
            record,
          },
        });
      }
      const savedRecord = {
        ...record,
        projectId: saved.projectId,
        projectName: saved.summary.projectName,
        updatedAt: saved.savedAt,
      };
      setCurrentProjectId(saved.projectId);
      setCurrentProjectName(saved.summary.projectName);
      setProgressRecord(savedRecord);
      setLastSavedAt(saved.savedAt);
      setProjectDirty(false);
      setProjectList((prev) => {
        const next = prev.filter((item) => item.projectId !== saved.summary.projectId);
        return [saved.summary, ...next].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
      });
      if (!options?.silent) {
        message.success(saved.message || "学习记录已保存");
      }
    } catch (error) {
      const text = String(error);
      setProgressSaveError(text);
      message.error(`学习记录保存失败：${text}`);
      if (options?.throwOnError) {
        throw error;
      }
    } finally {
      setProgressSaving(false);
    }
  }

  function restoreProgress(record: LearningProgressRecord) {
    form.setFieldsValue({
      learningAssistantRoot: record.goal.learningAssistantRoot || DEFAULT_ENGINE_ROOT,
      courseName: record.goal.course || FIXED_COURSE_NAME,
      learningGoal: record.goal.learningGoal || "系统学习",
      learningCycle: record.goal.learningCycle || GOAL_CYCLE_MAP["系统学习"],
      dailyStudyHours: parseStudyHours(record.goal.dailyTime),
      currentLevel:
        record.goal.currentLevel || "基础一般：掌握部分概念，但缺少系统复习",
      finalGoal: FINAL_GOAL_OPTIONS.includes(record.goal.finalGoal)
        ? record.goal.finalGoal
        : CUSTOM_FINAL_GOAL,
      finalGoalCustom: FINAL_GOAL_OPTIONS.includes(record.goal.finalGoal)
        ? undefined
        : record.goal.finalGoal,
    });
    setResult(record.plan);
    setStageResources(record.stageResources ?? {});
    setStageKbStates(record.stageKbStates ?? {});
    setStageQuizzes(record.stageQuizzes ?? {});
    setWrongQuestionReviewPrompts(record.wrongQuestionReviewPrompts ?? []);
    setActiveWrongQuestionReviewPromptId(null);
    setWrongQuestionReviewExpanded(false);
    wrongQuestionPromptSessionRef.current = new Set();
    setPlanKbContext(record.planKbContext ?? null);
    setAdjustResults(record.adjustResults ?? {});
    setProgressRecord(record);
    setCurrentProjectId(record.projectId ?? null);
    setCurrentProjectName(record.projectName ?? inferProjectNameFromValues(record.goal));
    const restoredThresholds = normalizeAdjustmentThresholds(record.adjustmentThresholds);
    setAdjustmentThresholds(restoredThresholds);
    setThresholdDraft(restoredThresholds);
    setLastSavedAt(record.updatedAt);
    setProjectDirty(false);
    setProgressSaveError(null);
  }

  function markProjectDirty() {
    setProjectDirty(true);
  }

  async function refreshProjectList(openManagerWhenLoaded = false) {
    if (!isTauriRuntime()) {
      if (openManagerWhenLoaded) setProjectManagerOpen(true);
      return;
    }
    setProgressLoading(true);
    try {
      const listed = await invoke<LearningProjectListResult>("learning_project_list");
      setProjectList(listed.projects);
      if (openManagerWhenLoaded || !currentProjectId) {
        setProjectManagerOpen(true);
      }
    } catch (error) {
      const text = String(error);
      setProgressSaveError(text);
      message.warning(`读取学习项目列表失败：${text}`);
    } finally {
      setProgressLoading(false);
    }
  }

  async function confirmUnsavedChanges() {
    if (!projectDirtyRef.current) return true;
    setUnsavedConfirmOpen(true);
    return new Promise<boolean>((resolve) => {
      unsavedResolverRef.current = resolve;
    });
  }

  function resolveUnsavedConfirm(value: boolean) {
    setUnsavedConfirmOpen(false);
    unsavedResolverRef.current?.(value);
    unsavedResolverRef.current = null;
  }

  async function handleUnsavedSaveAndContinue() {
    try {
      await saveProgressRecord(buildProgressRecord(), { throwOnError: true });
      resolveUnsavedConfirm(true);
    } catch {
      // saveProgressRecord already shows the concrete error.
    }
  }

  function handleUnsavedDiscardAndContinue() {
    setProjectDirty(false);
    resolveUnsavedConfirm(true);
  }

  async function handleOpenProject(projectId: string) {
    const canContinue = await confirmUnsavedChanges();
    if (!canContinue) return;
    setProgressLoading(true);
    try {
      const loaded = await invoke<LearningProjectLoadResult>("learning_project_load", {
        input: { projectId },
      });
      if (loaded.error || !loaded.project) {
        message.warning(loaded.error || loaded.message);
        return;
      }
      restoreProgress(loaded.project);
      if (loaded.summary) {
        setProjectList((prev) => {
          const next = prev.filter((item) => item.projectId !== loaded.summary?.projectId);
          return [loaded.summary!, ...next].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
        });
      }
      setProjectManagerOpen(false);
      message.success(loaded.message || "学习项目已打开");
    } catch (error) {
      message.error(`打开学习项目失败：${String(error)}`);
    } finally {
      setProgressLoading(false);
    }
  }

  async function handleCreateProject() {
    const canContinue = await confirmUnsavedChanges();
    if (!canContinue) return;
    const values = form.getFieldsValue();
    const defaultName = inferProjectNameFromValues(values);
    const projectName = window.prompt("请输入学习项目名称", defaultName)?.trim();
    if (!projectName) {
      message.warning("项目名称不能为空");
      return;
    }
    try {
      const defaults = getDefaultLearningValues();
      const defaultThresholds = { ...DEFAULT_ADJUSTMENT_THRESHOLDS };
      const record: LearningProgressRecord = {
        ...buildProgressRecord({ plan: null, stageQuizzes: {}, stageResources: {}, stageKbStates: {}, adjustResults: {}, adjustments: [], createdAt: nowText() }),
        projectName,
        adjustmentThresholds: defaultThresholds,
        goal: {
          course: defaults.courseName,
          learningGoal: defaults.learningGoal,
          learningCycle: defaults.learningCycle,
          dailyTime: formatStudyHours(defaults.dailyStudyHours),
          currentLevel: defaults.currentLevel,
          finalGoal: defaults.finalGoal,
          learningAssistantRoot: defaults.learningAssistantRoot,
        },
        plan: null,
        stageStatuses: [],
        stageResources: {},
        stageKbStates: {},
        stageQuizzes: {},
        planKbContext: null,
        testRecords: [],
        wrongQuestionReviewPrompts: [],
        adjustments: [],
        adjustResults: {},
      };
      const created = await invoke<LearningProjectSaveResult>("learning_project_create", {
        input: { projectName, record },
      });
      form.setFieldsValue(defaults);
      setResult(null);
      setStageResources({});
      setStageKbStates({});
      setStageQuizzes({});
      setWrongQuestionReviewPrompts([]);
      setActiveWrongQuestionReviewPromptId(null);
      setWrongQuestionReviewExpanded(false);
      wrongQuestionPromptSessionRef.current = new Set();
      setAdjustResults({});
      setPlanKbContext(null);
      setAdjustmentThresholds(defaultThresholds);
      setThresholdDraft(defaultThresholds);
      setProgressRecord({ ...record, projectId: created.projectId, updatedAt: created.savedAt });
      setCurrentProjectId(created.projectId);
      setCurrentProjectName(created.summary.projectName);
      setLastSavedAt(created.savedAt);
      setProjectDirty(false);
      setProjectList((prev) => [created.summary, ...prev]);
      setProjectManagerOpen(false);
      message.success("学习项目已创建");
    } catch (error) {
      message.error(`创建学习项目失败：${String(error)}`);
    }
  }

  async function handleManualSaveProject() {
    try {
      await saveProgressRecord(buildProgressRecord());
    } catch {
      // saveProgressRecord already shows the concrete error.
    }
  }

  async function handleSaveAdjustmentThresholds() {
    const nextThresholds = {
      relearnThreshold: Number(thresholdDraft.relearnThreshold),
      excellentThreshold: Number(thresholdDraft.excellentThreshold),
    };
    if (!validateAdjustmentThresholds(nextThresholds)) {
      message.error("重学阈值必须小于优秀阈值，且两个值都必须在0到100之间。");
      return;
    }

    const normalized = normalizeAdjustmentThresholds(nextThresholds);
    setAdjustmentThresholds(normalized);
    setThresholdDraft(normalized);
    markProjectDirty();
    await saveProgressRecord(
      buildProgressRecord({
        adjustmentThresholds: normalized,
      }),
    );
    message.success("本地调整阈值已保存到当前学习项目");
  }

  async function handleResetAdjustmentThresholds() {
    const defaults = { ...DEFAULT_ADJUSTMENT_THRESHOLDS };
    setAdjustmentThresholds(defaults);
    setThresholdDraft(defaults);
    markProjectDirty();
    await saveProgressRecord(
      buildProgressRecord({
        adjustmentThresholds: defaults,
      }),
    );
    message.success("已恢复默认阈值 60 / 80");
  }

  function getWrongQuestionReviewPromptForStage(stageIndex: number) {
    return [...wrongQuestionReviewPrompts]
      .reverse()
      .find(
        (prompt) =>
          prompt.targetStageIndex === stageIndex &&
          prompt.wrongQuestions.length > 0,
      );
  }

  function getAutoWrongQuestionReviewPrompt(stageIndex: number) {
    return wrongQuestionReviewPrompts.find(
      (prompt) =>
        prompt.targetStageIndex === stageIndex &&
        prompt.fromStageIndex === stageIndex - 1 &&
        prompt.wrongQuestions.length > 0 &&
        !prompt.reviewed &&
        !prompt.dismissed &&
        !wrongQuestionPromptSessionRef.current.has(prompt.id),
    );
  }

  async function saveWrongQuestionReviewPrompts(nextPrompts: WrongQuestionReviewPrompt[]) {
    setWrongQuestionReviewPrompts(nextPrompts);
    markProjectDirty();
    await saveProgressRecord(
      buildProgressRecord({
        wrongQuestionReviewPrompts: nextPrompts,
      }),
      { silent: true },
    );
  }

  async function updateWrongQuestionReviewPrompt(
    promptId: string,
    updater: (prompt: WrongQuestionReviewPrompt) => WrongQuestionReviewPrompt,
  ) {
    const nextPrompts = wrongQuestionReviewPrompts.map((prompt) =>
      prompt.id === promptId ? updater(prompt) : prompt,
    );
    await saveWrongQuestionReviewPrompts(nextPrompts);
  }

  async function openWrongQuestionReviewPrompt(
    prompt: WrongQuestionReviewPrompt,
    options?: { auto?: boolean; expanded?: boolean },
  ) {
    setActiveWrongQuestionReviewPromptId(prompt.id);
    setWrongQuestionReviewExpanded(Boolean(options?.expanded));
    if (options?.auto) {
      wrongQuestionPromptSessionRef.current.add(prompt.id);
    }
    if (!prompt.shown || options?.auto) {
      const shownAt = nowText();
      await updateWrongQuestionReviewPrompt(prompt.id, (item) => ({
        ...item,
        shown: true,
        lastShownAt: shownAt,
        updatedAt: shownAt,
      }));
    }
  }

  async function handleWrongQuestionReviewLater() {
    const prompt = activeWrongQuestionReviewPrompt;
    if (!prompt) return;
    wrongQuestionPromptSessionRef.current.add(prompt.id);
    setActiveWrongQuestionReviewPromptId(null);
    setWrongQuestionReviewExpanded(false);
    await updateWrongQuestionReviewPrompt(prompt.id, (item) => ({
      ...item,
      userDecision: "later",
      updatedAt: nowText(),
    }));
  }

  async function handleDismissWrongQuestionReview() {
    const prompt = activeWrongQuestionReviewPrompt;
    if (!prompt) return;
    wrongQuestionPromptSessionRef.current.add(prompt.id);
    setActiveWrongQuestionReviewPromptId(null);
    setWrongQuestionReviewExpanded(false);
    await updateWrongQuestionReviewPrompt(prompt.id, (item) => ({
      ...item,
      dismissed: true,
      userDecision: "dismissed",
      updatedAt: nowText(),
    }));
  }

  async function handleMarkWrongQuestionReviewed(questionId: string) {
    const prompt = activeWrongQuestionReviewPrompt;
    if (!prompt) return;
    await updateWrongQuestionReviewPrompt(prompt.id, (item) => {
      const reviewedQuestionIds = uniqueValues([...item.reviewedQuestionIds, questionId]);
      const reviewed = reviewedQuestionIds.length >= item.wrongQuestions.length;
      return {
        ...item,
        reviewedQuestionIds,
        reviewed,
        userDecision: reviewed ? "reviewed" : item.userDecision,
        updatedAt: nowText(),
      };
    });
  }

  async function handleCompleteWrongQuestionReview() {
    const prompt = activeWrongQuestionReviewPrompt;
    if (!prompt) return;
    const allQuestionIds = prompt.wrongQuestions.map((item) => item.questionId);
    wrongQuestionPromptSessionRef.current.add(prompt.id);
    setActiveWrongQuestionReviewPromptId(null);
    setWrongQuestionReviewExpanded(false);
    await updateWrongQuestionReviewPrompt(prompt.id, (item) => ({
      ...item,
      reviewed: true,
      reviewedQuestionIds: allQuestionIds,
      userDecision: "reviewed",
      updatedAt: nowText(),
    }));
  }

  async function handleRenameProject(project: LearningProjectSummary) {
    const projectName = window.prompt("请输入新的项目名称", project.projectName)?.trim();
    if (!projectName) return;
    try {
      const renamed = await invoke<LearningProjectSaveResult>("learning_project_rename", {
        input: { projectId: project.projectId, projectName },
      });
      setProjectList((prev) =>
        prev.map((item) => (item.projectId === project.projectId ? renamed.summary : item)),
      );
      if (currentProjectId === project.projectId) {
        setCurrentProjectName(projectName);
        setProgressRecord((prev) => (prev ? { ...prev, projectName } : prev));
      }
      message.success("项目已重命名");
    } catch (error) {
      message.error(`重命名失败：${String(error)}`);
    }
  }

  async function handleDuplicateProject(project: LearningProjectSummary) {
    const projectName = window.prompt("请输入新项目名称", `${project.projectName} 副本`)?.trim();
    if (!projectName) return;
    try {
      const duplicated = await invoke<LearningProjectSaveResult>("learning_project_duplicate", {
        input: { projectId: project.projectId, projectName },
      });
      setProjectList((prev) => [duplicated.summary, ...prev]);
      message.success("项目已复制，可在列表中打开");
    } catch (error) {
      message.error(`复制项目失败：${String(error)}`);
    }
  }

  async function handleDeleteProject(project: LearningProjectSummary) {
    const canContinue = currentProjectId === project.projectId ? await confirmUnsavedChanges() : true;
    if (!canContinue) return;
    Modal.confirm({
      title: `删除学习项目：${project.projectName}`,
      content: "删除后不会影响其他学习项目，但该项目记录将不再显示。",
      okText: "确认删除",
      okButtonProps: { danger: true },
      cancelText: "取消",
      onOk: async () => {
        try {
          const deleted = await invoke<LearningProjectDeleteResult>("learning_project_delete", {
            input: { projectId: project.projectId },
          });
          setProjectList((prev) => prev.filter((item) => item.projectId !== project.projectId));
          if (currentProjectId === project.projectId) {
            setCurrentProjectId(null);
            setCurrentProjectName("");
            setProgressRecord(null);
            setLastSavedAt("");
            setProjectDirty(false);
            setResult(null);
            setStageResources({});
            setStageKbStates({});
            setStageQuizzes({});
            setWrongQuestionReviewPrompts([]);
            setActiveWrongQuestionReviewPromptId(null);
            setWrongQuestionReviewExpanded(false);
            wrongQuestionPromptSessionRef.current = new Set();
            setAdjustResults({});
            setPlanKbContext(null);
            setAdjustmentThresholds({ ...DEFAULT_ADJUSTMENT_THRESHOLDS });
            setThresholdDraft({ ...DEFAULT_ADJUSTMENT_THRESHOLDS });
            form.setFieldsValue(getDefaultLearningValues());
          }
          message.success(deleted.message);
        } catch (error) {
          message.error(`删除项目失败：${String(error)}`);
        }
      },
    });
  }

  useEffect(() => {
    void refreshProjectList(true);
    void loadDocumentSources();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    projectDirtyRef.current = projectDirty;
  }, [projectDirty]);

  useEffect(() => {
    const handleBeforeUnload = (event: BeforeUnloadEvent) => {
      if (!projectDirtyRef.current) return;
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => window.removeEventListener("beforeunload", handleBeforeUnload);
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;
    void getCurrentWindow()
      .onCloseRequested(async (event) => {
        if (!projectDirtyRef.current) return;
        event.preventDefault();
        const canClose = await confirmUnsavedChanges();
        if (canClose) {
          await getCurrentWindow().destroy();
        }
      })
      .then((dispose) => {
        unlisten = dispose;
      });
    return () => {
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!result?.stages.length) return;
    const prompt = getAutoWrongQuestionReviewPrompt(inferredCurrentStageIndex);
    if (!prompt) return;
    void openWrongQuestionReviewPrompt(prompt, { auto: true });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [inferredCurrentStageIndex, result?.stages.length, wrongQuestionReviewPrompts]);

  function confirmOverwriteExistingPlan() {
    if (!result?.stages.length && !progressRecord?.plan?.stages.length) {
      return Promise.resolve(true);
    }

    return new Promise<boolean>((resolve) => {
      Modal.confirm({
        title: "生成新学习计划？",
        content:
          "当前已有学习记录。生成新计划会覆盖最近一次学习计划和阶段状态，但旧记录会在你确认后才被覆盖。",
        okText: "继续生成",
        cancelText: "取消",
        onOk: () => resolve(true),
        onCancel: () => resolve(false),
      });
    });
  }

  function handleStartNewPlan() {
    Modal.confirm({
      title: "开始新计划？",
      content: "这会清空当前页面中的计划和测试状态，但不会删除已保存的学习记录。",
      okText: "开始新计划",
      cancelText: "取消",
      onOk: () => {
        form.setFieldsValue(getDefaultLearningValues());
        setResult(null);
        setStageResources({});
        setStageKbStates({});
        setStageQuizzes({});
        setWrongQuestionReviewPrompts([]);
        setActiveWrongQuestionReviewPromptId(null);
        setWrongQuestionReviewExpanded(false);
        wrongQuestionPromptSessionRef.current = new Set();
        setAdjustResults({});
        setPlanKbContext(null);
        setProgressSaveError(null);
        message.success("已切换到新计划输入状态");
      },
    });
  }

  async function handleClearProgress() {
    Modal.confirm({
      title: "清除当前学习记录？",
      content: "清除后将无法自动恢复最近一次学习计划、测试成绩和薄弱点。",
      okText: "确认清除",
      okButtonProps: { danger: true },
      cancelText: "取消",
      onOk: async () => {
        try {
          if (isTauriRuntime()) {
            const cleared = await invoke<LearningProgressClearResult>(
              "learning_progress_clear_latest",
            );
            message.success(cleared.message);
          }
          setProgressRecord(null);
          setLastSavedAt("");
          setProgressSaveError(null);
          setResult(null);
          setStageResources({});
          setStageKbStates({});
          setStageQuizzes({});
          setWrongQuestionReviewPrompts([]);
          setActiveWrongQuestionReviewPromptId(null);
          setWrongQuestionReviewExpanded(false);
          wrongQuestionPromptSessionRef.current = new Set();
          setAdjustResults({});
          setPlanKbContext(null);
        } catch (error) {
          message.error(`清除学习记录失败：${String(error)}`);
        }
      },
    });
  }

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
        dailyStudyHours: 1,
        currentLevel: "",
        finalGoal: "",
        selectedDocumentSourceIds: [],
        selectedLearningSources: [],
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

  async function openAiConfigModal() {
    setApiConfigOpen(true);

    if (!isTauriRuntime()) {
      const fallbackStatus: LearningAssistantAiConfigStatus = {
        apiBase: "https://api.deepseek.com",
        model: "deepseek-chat",
        hasApiKey: false,
        source: "browser",
      };
      setApiConfigStatus(fallbackStatus);
      apiForm.setFieldsValue({
        apiBase: fallbackStatus.apiBase,
        model: fallbackStatus.model,
        apiKey: "",
      });
      return;
    }

    setApiConfigLoading(true);
    try {
      const status = await invoke<LearningAssistantAiConfigStatus>(
        "learning_assistant_ai_get_config",
      );
      setApiConfigStatus(status);
      apiForm.setFieldsValue({
        apiBase: status.apiBase,
        model: status.model,
        apiKey: "",
      });
    } catch (error) {
      message.error(`读取 API 配置失败：${String(error)}`);
    } finally {
      setApiConfigLoading(false);
    }
  }

  async function handleSaveAiConfig() {
    try {
      const values = await apiForm.validateFields(["apiBase", "model"]);
      const apiKey = String(apiForm.getFieldValue("apiKey") ?? "");

      if (!isTauriRuntime()) {
        message.info("浏览器调试模式无法保存 Tauri 后端 API 配置，请在桌面端测试。");
        return;
      }

      setApiConfigSaving(true);
      const status = await invoke<LearningAssistantAiConfigStatus>(
        "learning_assistant_ai_save_config",
        {
          input: {
            apiBase: values.apiBase,
            apiKey,
            model: values.model,
          },
        },
      );

      setApiConfigStatus(status);
      apiForm.setFieldValue("apiKey", "");
      setApiConfigOpen(false);
      message.success("AI 助学理解 API 已保存，本次运行立即生效");
    } catch (error) {
      message.error(String(error));
    } finally {
      setApiConfigSaving(false);
    }
  }

  async function handleClearAiConfig() {
    if (!isTauriRuntime()) return;
    setApiConfigSaving(true);
    try {
      const status = await invoke<LearningAssistantAiConfigStatus>(
        "learning_assistant_ai_clear_config",
      );
      setApiConfigStatus(status);
      apiForm.setFieldsValue({
        apiBase: status.apiBase,
        model: status.model,
        apiKey: "",
      });
      message.success("已清除用户配置，将按环境变量或本地模板继续运行");
    } catch (error) {
      message.error(String(error));
    } finally {
      setApiConfigSaving(false);
    }
  }

  async function hasConfiguredAdjustmentApi() {
    if (!isTauriRuntime()) return false;
    if (apiConfigStatus?.hasApiKey) return true;
    try {
      const status = await invoke<LearningAssistantAiConfigStatus>(
        "learning_assistant_ai_get_config",
      );
      setApiConfigStatus(status);
      return status.hasApiKey;
    } catch {
      return false;
    }
  }

  async function buildPluginEnhancedInput(
    values: LearningAssistantFormValues,
    feature: string,
  ): Promise<{
    input: ReturnType<typeof buildCommandInput>;
    before: PluginPipelineBeforeResult;
  }> {
    const baseInput = buildCommandInput(values, selectedLearningSources);
    const before = await runPluginPipelineBeforeModel({
      scene: "learning",
      feature,
      userRole: "student",
      input: baseInput,
      prompt: "",
      metadata: {
        courseName: FIXED_COURSE_NAME,
        learningGoal: baseInput.learningGoal,
      },
    });
    const contextParts: string[] = [];
    if (before.prompt.trim()) {
      contextParts.push(before.prompt.trim());
    }
    if (
      typeof before.input === "string" &&
      before.input.trim() &&
      before.input.trim() !== JSON.stringify(baseInput)
    ) {
      contextParts.push(`插件处理后的输入：\n${before.input.trim()}`);
    }
    setPluginEnhancementStatus({
      executed: before.executedContributionIds,
      warnings: before.warnings,
    });
    return {
      input: buildCommandInput(
        values,
        selectedLearningSources,
        contextParts.length ? contextParts.join("\n\n") : undefined,
      ),
      before,
    };
  }

  async function recordPluginAfterModel(
    before: PluginPipelineBeforeResult,
    output: LearningAssistantPlanResult,
  ) {
    try {
      const after = await runPluginPipelineAfterModel(before, JSON.stringify(output));
      setPluginEnhancementStatus({
        executed: after.executedContributionIds,
        warnings: after.warnings,
      });
    } catch (error) {
      setPluginEnhancementStatus((prev) => ({
        executed: prev?.executed ?? before.executedContributionIds,
        warnings: [...(prev?.warnings ?? before.warnings), String(error)],
      }));
    }
  }

  async function handleUnderstand() {
    try {
      const values = await form.validateFields();
      setUnderstandingLoading(true);
      const enhanced = await buildPluginEnhancedInput(values, "goal-understanding");
      const understood = await callLearningAssistant<LearningAssistantPlanResult>(
        "learning_assistant_understand",
        enhanced.input,
      );
      await recordPluginAfterModel(enhanced.before, understood);
      setResult(understood);
      markProjectDirty();
      if (understood.message?.includes("fallback")) {
        message.warning(understood.message);
      } else {
        message.success(understood.message || "目标理解已生成");
      }
    } catch (error) {
      message.error(String(error));
    } finally {
      setUnderstandingLoading(false);
    }
  }

  async function searchKbForPlan(values: LearningAssistantFormValues) {
    const input = buildCommandInput(values, selectedLearningSources);

    if (!isTauriRuntime()) {
      const fallback: LearningKbSearchResult = {
        results: [],
        message: "浏览器调试模式暂不能读取本地 Excel，请在 Tauri 桌面端运行。",
      };
      setPlanKbContext(fallback);
      return fallback;
    }

    const searched = await invoke<LearningKbSearchResult>("learning_kb_search", {
      input: {
        course: input.courseName,
        query: [
          input.learningGoal,
          input.learningCycle,
          input.dailyTime,
          input.currentLevel,
          input.finalGoal,
        ].join(" "),
        stageName: "生成学习计划",
        stageIndex: 0,
        stageGoal: input.finalGoal,
        learningTasks: [],
        resourceTasks: [],
        practiceTasks: [],
        checkTasks: [],
        knowledgePoints: [input.learningGoal, input.currentLevel, input.finalGoal],
        topK: 10,
        documentSourceIds: selectedDocumentSourceIds,
      },
    });
    setPlanKbContext(searched);
    return searched;
  }

  async function handleGeneratePlan() {
    try {
      const canOverwrite = await confirmOverwriteExistingPlan();
      if (!canOverwrite) return;

      const values = await form.validateFields();
      if (!selectedLearningSources.length) {
        message.warning("请至少选择一个可用的助学数据文件");
        return;
      }
      if (selectedLearningSources.every((source) => source.importanceLevel === "reference")) {
        message.warning("所选文件不能全部设为“仅供参考”");
        return;
      }
      setPlanLoading(true);
      let latestPlanKbContext: LearningKbSearchResult | null = null;
      try {
        const kbContext = await searchKbForPlan(values);
        latestPlanKbContext = kbContext;
        if (kbContext.results.length) {
          message.info(
            `已检索到 ${kbContext.results.length} 条本地知识库内容，后续可传入大模型作为计划生成上下文。`,
          );
        } else if (kbContext.message) {
          message.info(kbContext.message);
        }
      } catch (error) {
        setPlanKbContext({
          results: [],
          message: "",
          warnings: [String(error)],
        });
        message.warning(`本地知识库查询失败，已继续生成学习计划：${String(error)}`);
      }
      const enhanced = await buildPluginEnhancedInput(values, "learning-plan-generation");
      const generated = await callLearningAssistant<LearningAssistantPlanResult>(
        "learning_assistant_generate_plan",
        enhanced.input,
      );
      await recordPluginAfterModel(enhanced.before, generated);
      const emptyResources: Record<number, StageResourceState> = {};
      const emptyKbStates: Record<number, StageKbState> = {};
      const emptyQuizzes: Record<number, StageQuizState> = {};
      const emptyAdjustResults: Record<number, LearningPlanAdjustResult> = {};
      setResult(generated);
      setStageResources(emptyResources);
      setStageKbStates(emptyKbStates);
      setStageQuizzes(emptyQuizzes);
      setWrongQuestionReviewPrompts([]);
      setActiveWrongQuestionReviewPromptId(null);
      setWrongQuestionReviewExpanded(false);
      wrongQuestionPromptSessionRef.current = new Set();
      setAdjustResults(emptyAdjustResults);
      markProjectDirty();
      message.success("学习计划已生成");
      await saveProgressRecord(
        buildProgressRecord({
          plan: generated,
          stageResources: emptyResources,
          stageKbStates: emptyKbStates,
          stageQuizzes: emptyQuizzes,
          wrongQuestionReviewPrompts: [],
          planKbContext: latestPlanKbContext,
          adjustResults: emptyAdjustResults,
          adjustments: [],
          createdAt: nowText(),
        }),
      );
    } catch (error) {
      message.error(String(error));
    } finally {
      setPlanLoading(false);
    }
  }

  function openLocalRelearnPrompt(index: number, scoreResult: LearningQuizScoreResult) {
    setLocalAdjustmentPrompt({
      stageIndex: index,
      scoreResult,
      previewVisible: false,
    });
  }

  async function updateLocalAdjustmentDecision(
    index: number,
    decision: "pending" | "accepted" | "declined",
    fields?: Partial<LearningQuizScoreResult>,
  ) {
    const quiz = stageQuizzes[index];
    const scoreResult = quiz?.scoreResult;
    if (!quiz || !scoreResult) return null;

    const decidedAt = nowText();
    const nextScoreResult: LearningQuizScoreResult = {
      ...scoreResult,
      adjustmentPromptShown: true,
      localAdjustmentDecision: decision,
      localAdjustmentDecidedAt: decision === "pending" ? scoreResult.localAdjustmentDecidedAt : decidedAt,
      ...fields,
    };
    const nextStageQuizzes = {
      ...stageQuizzes,
      [index]: {
        ...quiz,
        scoreResult: nextScoreResult,
      },
    };
    setStageQuizzes(nextStageQuizzes);
    markProjectDirty();
    await saveProgressRecord(
      buildProgressRecord({
        stageQuizzes: nextStageQuizzes,
      }),
    );
    return nextScoreResult;
  }

  async function handleDeclineLocalRelearnAdjustment(index: number) {
    await updateLocalAdjustmentDecision(index, "declined");
    setLocalAdjustmentPrompt(null);
    message.info("你已暂不采用重新学习建议，可稍后手动调整计划。");
  }

  async function handleAcceptLocalRelearnAdjustment(index: number) {
    if (!result?.stages.length) return;
    const quiz = stageQuizzes[index];
    const scoreResult = quiz?.scoreResult;
    if (!quiz || !scoreResult) return;

    if (
      scoreResult.localAdjustmentDecision === "accepted" &&
      adjustResults[index]?.source === "local_rule"
    ) {
      setLocalAdjustmentPrompt(null);
      message.info("本次测试的本地重新学习方案已经采用，无需重复添加任务。");
      return;
    }

    const adjustedAt = nowText();
    const beforePlan = result;
    const beforeStages = result.stages.map(copyLearningStage);
    const adjusted = buildLocalRelearnAdjustment({
      plan: result,
      stageIndex: index,
      quiz,
      scoreResult,
      adjustedAt,
      thresholds: adjustmentThresholds,
    });
    const nextPlan: LearningAssistantPlanResult = {
      ...result,
      stages: adjusted.stages,
      message: `学习计划已调整：${adjusted.conclusion}`,
    };
    const nextAdjustResult: LearningPlanAdjustResult = {
      ...adjusted,
      beforeStages,
      adjustedAt,
    };
    const nextScoreResult: LearningQuizScoreResult = {
      ...scoreResult,
      adjustmentPromptShown: true,
      localAdjustmentDecision: "accepted",
      localAdjustmentDecidedAt: adjustedAt,
      localAdjustmentReason: adjusted.reason,
      localAdjustmentSource: "local_rule",
      canGoNext: false,
    };
    const nextStageQuizzes = {
      ...stageQuizzes,
      [index]: {
        ...quiz,
        scoreResult: nextScoreResult,
      },
    };
    const nextAdjustResults = {
      ...adjustResults,
      [index]: nextAdjustResult,
    };
    const adjustment: LearningPlanAdjustmentSnapshot = {
      beforePlan,
      afterPlan: nextPlan,
      reason: adjusted.reason,
      adjustedAt,
      source: "local_rule",
      needRetest: true,
    };
    const nextAdjustments = [...(progressRecord?.adjustments ?? []), adjustment];

    setResult(nextPlan);
    setStageQuizzes(nextStageQuizzes);
    setAdjustResults(nextAdjustResults);
    markProjectDirty();
    await saveProgressRecord(
      buildProgressRecord({
        plan: nextPlan,
        stageQuizzes: nextStageQuizzes,
        adjustResults: nextAdjustResults,
        adjustments: nextAdjustments,
      }),
    );
    setLocalAdjustmentPrompt(null);
    message.success("已采用本地规则重新学习方案，后续阶段未锁定。");
  }

  async function handleAdjustPlan(index: number) {
    if (!result?.stages.length) return;

    const quiz = stageQuizzes[index];
    const scoreResult = quiz?.scoreResult;
    if (!scoreResult) {
      message.warning("请先完成本阶段测试并提交评分");
      return;
    }
    if (adjustingStages[index]) return;

    const percentage =
      scoreResult.percentage ?? getScorePercentage(scoreResult.totalScore, scoreResult.maxScore);
    const currentThresholds = normalizeAdjustmentThresholds(adjustmentThresholds);
    if (percentage < currentThresholds.relearnThreshold) {
      openLocalRelearnPrompt(index, {
        ...scoreResult,
        percentage,
        adjustmentPromptShown: true,
        localAdjustmentDecision: scoreResult.localAdjustmentDecision ?? "pending",
      });
      return;
    }

    const canUseApiAdjustment = await hasConfiguredAdjustmentApi();
    if (!canUseApiAdjustment) {
      message.info("当前分数已达到基本掌握，本地规则不自动修改计划；如需 AI 优化调整，可先配置 API。");
      return;
    }

    const values = form.getFieldsValue();
    const beforePlan = result;
    const beforeStages = result.stages.map(copyLearningStage);

    setAdjustingStages((prev) => ({ ...prev, [index]: true }));
    try {
      const adjusted = await invoke<LearningPlanAdjustResult>("learning_assistant_adjust_plan", {
        input: {
          courseName: String(values.courseName ?? FIXED_COURSE_NAME),
          currentLevel: String(values.currentLevel ?? ""),
          finalGoal:
            values.finalGoal === CUSTOM_FINAL_GOAL
              ? String(values.finalGoalCustom ?? "").trim()
              : String(values.finalGoal ?? "").trim(),
          dailyTime: String(values.dailyTime ?? ""),
          learningCycle: String(values.learningCycle ?? ""),
          stageIndex: index,
          stages: result.stages,
          score: scoreResult.totalScore,
          maxScore: scoreResult.maxScore,
          masteryLevel: scoreResult.level,
          weakPoints: scoreResult.weakPoints,
          missingKeywords: scoreResult.missingKeywords,
          wrongKnowledgePoints: getWrongKnowledgePoints(quiz),
          feedback: scoreResult.feedback,
          reviewSuggestions: scoreResult.suggestions,
        },
      });

      const adjustedAt = nowText();
      const nextPlan: LearningAssistantPlanResult = {
        ...result,
        stages: adjusted.stages,
        message: `学习计划已调整：${adjusted.conclusion}`,
      };
      const nextAdjustResult: LearningPlanAdjustResult = {
        ...adjusted,
        beforeStages,
        adjustedAt,
      };
      const nextAdjustResults = {
        ...adjustResults,
        [index]: nextAdjustResult,
      };
      const nextStageQuizzes = {
        ...stageQuizzes,
        [index]: {
          ...quiz,
          scoreResult: {
            ...scoreResult,
            canGoNext: adjusted.canAdvance,
          },
        },
      };
      const adjustment: LearningPlanAdjustmentSnapshot = {
        beforePlan,
        afterPlan: nextPlan,
        reason: adjusted.reason,
        adjustedAt,
        source: adjusted.source,
        needRetest: adjusted.needRetest,
      };
      const nextAdjustments = [...(progressRecord?.adjustments ?? []), adjustment];

      setResult(nextPlan);
      setStageQuizzes(nextStageQuizzes);
      setAdjustResults(nextAdjustResults);
      markProjectDirty();
      await saveProgressRecord(
        buildProgressRecord({
          plan: nextPlan,
          stageQuizzes: nextStageQuizzes,
          adjustResults: nextAdjustResults,
          adjustments: nextAdjustments,
        }),
      );

      message.success(
        adjusted.source === "spark" ? "已使用讯飞星火调整学习计划" : "已使用本地规则调整学习计划",
      );
    } catch (error) {
      message.error(`调整计划失败：${String(error)}`);
    } finally {
      setAdjustingStages((prev) => ({ ...prev, [index]: false }));
    }
  }

  async function handleUndoLastAdjustment(index: number) {
    const adjustment = adjustResults[index];
    if (!result || !adjustment?.beforeStages?.length) return;

    const restoredPlan: LearningAssistantPlanResult = {
      ...result,
      stages: adjustment.beforeStages,
      message: "已撤销上一次计划调整",
    };
    const nextAdjustResults = { ...adjustResults };
    delete nextAdjustResults[index];
    const undoSnapshot: LearningPlanAdjustmentSnapshot = {
      beforePlan: result,
      afterPlan: restoredPlan,
      reason: `撤销阶段 ${index + 1} 的上一次动态计划调整。`,
      adjustedAt: nowText(),
      source: "undo",
      needRetest: false,
    };
    const nextAdjustments = [...(progressRecord?.adjustments ?? []), undoSnapshot];

    setResult(restoredPlan);
    setAdjustResults(nextAdjustResults);
    markProjectDirty();
    await saveProgressRecord(
      buildProgressRecord({
        plan: restoredPlan,
        adjustResults: nextAdjustResults,
        adjustments: nextAdjustments,
      }),
    );
    message.success("已撤销上一次计划调整");
  }

  async function handleAdjustPlanPlaceholder(index: number) {
    message.info(PLACEHOLDER_MESSAGE);
    if (!result) return;

    const adjustment: LearningPlanAdjustmentSnapshot = {
      beforePlan: result,
      afterPlan: result,
      reason: `阶段 ${index + 1} 调整计划入口已点击，当前版本尚未执行真实计划调整。`,
      adjustedAt: nowText(),
      source: "placeholder",
      needRetest: false,
    };
    await saveProgressRecord({
      ...buildProgressRecord(),
      adjustments: [...(progressRecord?.adjustments ?? []), adjustment],
    });
  }

  void handleAdjustPlanPlaceholder;

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
            : "当前尚未接入资源库，可先使用本地知识点资料进行学习。",
          error: null,
        },
      }));
      markProjectDirty();
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

      const nextStageResources = {
        ...stageResources,
        [index]: {
          loading: false,
          resources: recommended.resources,
          message: recommended.resources.length
            ? recommended.message || "已返回匹配资源"
            : "当前尚未接入资源库，可先使用本地知识点资料进行学习。",
          error: null,
        },
      };
      setStageResources(nextStageResources);
      markProjectDirty();
      await saveProgressRecord(
        buildProgressRecord({
          stageResources: nextStageResources,
        }),
      );
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

  async function handleSearchLocalKb(searchText = localKbQuery) {
    const values = form.getFieldsValue();
    const course = String(values.courseName ?? "").trim();
    const query = String(searchText ?? "").trim();

    if (!query) {
      message.warning("请输入要搜索的内容");
      return;
    }

    setLocalKbQuery(query);

    if (!isTauriRuntime()) {
      setLocalKbState({
        loading: false,
        results: [],
        message: "浏览器调试模式暂不能读取本地 Excel，请在 Tauri 桌面端运行。",
        error: null,
      });
      return;
    }

    setLocalKbState((prev) => ({
      loading: true,
      results: prev.results,
      message: "",
      error: null,
    }));

    try {
      const searched = await invoke<LearningKbSearchResult>("learning_kb_search", {
        input: {
          course,
          query,
          stageName: "",
          stageIndex: 0,
          stageGoal: "",
          learningTasks: [],
          resourceTasks: [],
          practiceTasks: [],
          checkTasks: [],
          knowledgePoints: [],
          topK: 5,
          documentSourceIds: selectedDocumentSourceIds,
        },
      });

      setLocalKbState({
        loading: false,
        results: searched.results,
        message: searched.message || "当前本地知识库暂无匹配内容",
        error: null,
      });
    } catch (error) {
      setLocalKbState({
        loading: false,
        results: [],
        message: "",
        error: `本地知识库查询失败：${String(error)}`,
      });
    }
  }

  async function handleSearchStageKb(stage: LearningAssistantStage, index: number) {
    const values = form.getFieldsValue();
    const course = String(values.courseName ?? "").trim();
    const knowledgePoints = buildStageKnowledgePoints(stage);

    if (!course) {
      message.warning("请先填写课程名称");
      return;
    }

    if (!isTauriRuntime()) {
      setStageKbStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          results: [],
          message: "浏览器调试模式暂不能读取本地 Excel，请在 Tauri 桌面端运行。",
          error: null,
        },
      }));
      markProjectDirty();
      return;
    }

    setStageKbStates((prev) => ({
      ...prev,
      [index]: {
        loading: true,
        results: prev[index]?.results ?? [],
        message: "",
        error: null,
      },
    }));

    try {
      const searched = await invoke<LearningKbSearchResult>("learning_kb_search", {
        input: {
          course,
          query: knowledgePoints.join(" "),
          stageName: stage.name,
          stageIndex: index + 1,
          stageGoal: stage.goal,
          learningTasks: stage.learningTasks,
          resourceTasks: stage.resourceTasks,
          practiceTasks: stage.practiceTasks,
          checkTasks: stage.checkTasks,
          knowledgePoints,
          topK: 5,
          documentSourceIds: selectedDocumentSourceIds,
        },
      });

      const nextStageKbStates = {
        ...stageKbStates,
        [index]: {
          loading: false,
          results: searched.results,
          message: searched.message || "当前本地知识库暂无与本阶段匹配的内容。",
          error: null,
        },
      };
      setStageKbStates(nextStageKbStates);
      markProjectDirty();
      await saveProgressRecord(
        buildProgressRecord({
          stageKbStates: nextStageKbStates,
        }),
      );
    } catch (error) {
      setStageKbStates((prev) => ({
        ...prev,
        [index]: {
          loading: false,
          results: [],
          message: "",
          error: `本地资料查询失败：${String(error)}`,
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
      markProjectDirty();
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
      markProjectDirty();
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
    markProjectDirty();
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
      const currentThresholds = normalizeAdjustmentThresholds(adjustmentThresholds);
      const rawScoreResult = isTauriRuntime()
        ? await invoke<LearningQuizScoreResult>("learning_quiz_score", {
            input: {
              stageName: result?.stages[index]?.name ?? `阶段 ${index + 1}`,
              stageIndex: index + 1,
              studentId: currentProjectId ?? undefined,
              questions: quiz.questions,
              answers,
            },
          })
        : scoreBrowserQuiz(quiz.questions, answers, currentThresholds);
      const normalizedScoreResult = normalizeQuizScoreResult(
        rawScoreResult,
        index,
        currentThresholds,
      );
      const scoreResult: LearningQuizScoreResult = shouldShowLowScorePrompt(
        normalizedScoreResult,
        currentThresholds,
      )
        ? {
            ...normalizedScoreResult,
            adjustmentPromptShown: true,
            localAdjustmentDecision: "pending",
          }
        : normalizedScoreResult;

      const updatedStageQuizzes = {
        ...stageQuizzes,
        [index]: {
          ...quiz,
          answers: quiz.answers,
          scoring: false,
          scoreResult,
          error: null,
        },
      };
      const nextReviewPrompt = buildNextStageWrongQuestionReviewPrompt({
        plan: result,
        stageIndex: index,
        quiz,
        scoreResult,
        thresholds: currentThresholds,
      });
      const nextWrongQuestionReviewPrompts = syncWrongQuestionReviewPrompts(
        wrongQuestionReviewPrompts,
        index,
        nextReviewPrompt,
      );

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
      setWrongQuestionReviewPrompts(nextWrongQuestionReviewPrompts);
      markProjectDirty();
      await saveProgressRecord(
        buildProgressRecord({
          stageQuizzes: updatedStageQuizzes,
          wrongQuestionReviewPrompts: nextWrongQuestionReviewPrompts,
        }),
      );
      if (shouldShowLowScorePrompt(normalizedScoreResult, currentThresholds)) {
        openLocalRelearnPrompt(index, scoreResult);
      }
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

  const localPromptStage = localAdjustmentPrompt
    ? result?.stages[localAdjustmentPrompt.stageIndex]
    : undefined;
  const localPromptQuiz = localAdjustmentPrompt
    ? stageQuizzes[localAdjustmentPrompt.stageIndex]
    : undefined;
  const localPromptWeakPoints =
    localAdjustmentPrompt && localPromptQuiz
      ? collectLocalAdjustmentWeakPoints(localPromptQuiz, localAdjustmentPrompt.scoreResult)
      : [];
  const localPromptPreview =
    localAdjustmentPrompt && result && localPromptQuiz
      ? buildLocalRelearnAdjustment({
          plan: result,
          stageIndex: localAdjustmentPrompt.stageIndex,
          quiz: localPromptQuiz,
          scoreResult: localAdjustmentPrompt.scoreResult,
          adjustedAt: nowText(),
          thresholds: adjustmentThresholds,
        })
      : null;

  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto max-w-6xl p-6">
        <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
          <div>
            <Title level={3} style={{ marginBottom: 4 }}>
              AI 助学
            </Title>
            <Text type="secondary">
              输入学习目标和基础信息，生成一次可执行的阶段学习计划。
            </Text>
          </div>
          <Space wrap>
            <Tag color={currentProjectId ? "blue" : "default"}>
              {currentProjectName || "未选择学习项目"}
            </Tag>
            <Tag color={progressSaveError ? "red" : lastSavedAt ? "green" : "default"}>
              {progressSaveError
                ? "保存失败"
                : progressSaving
                  ? "保存中"
                  : lastSavedAt
                    ? "已保存"
                    : "未保存"}
            </Tag>
            {lastSavedAt ? <Text type="secondary">最近保存：{lastSavedAt}</Text> : null}
            {projectDirty ? <Tag color="orange">有未保存修改</Tag> : null}
            <Button
              icon={<Save size={15} />}
              loading={progressSaving}
              onClick={handleManualSaveProject}
            >
              保存项目
            </Button>
            <Button icon={<PenLine size={15} />} onClick={handleCreateProject}>
              新建项目
            </Button>
            <Button
              icon={<History size={15} />}
              loading={progressLoading}
              onClick={() => {
                void refreshProjectList();
                setProjectManagerOpen(true);
              }}
            >
              学习项目
            </Button>
          </Space>
        </div>

        {progressSaveError ? (
          <Alert
            className="mb-4"
            type="warning"
            showIcon
            message="学习记录保存或读取异常"
            description={progressSaveError}
          />
        ) : null}

        {pluginEnhancementStatus &&
        (pluginEnhancementStatus.executed.length || pluginEnhancementStatus.warnings.length) ? (
          <Alert
            className="mb-4"
            type={pluginEnhancementStatus.warnings.length ? "warning" : "success"}
            showIcon
            message={`声明式学习增强：${pluginEnhancementStatus.executed.length} 个处理步骤已执行`}
            description={
              <Space direction="vertical" size={4}>
                {pluginEnhancementStatus.executed.length ? (
                  <Text>已启用：{pluginEnhancementStatus.executed.join("、")}</Text>
                ) : null}
                {pluginEnhancementStatus.warnings.map((warning) => (
                  <Text key={warning} type="secondary">
                    {warning}
                  </Text>
                ))}
              </Space>
            }
          />
        ) : null}

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

        <Card className="mb-4" title="本地调整阈值设置">
          <div className="space-y-3">
            <Space wrap align="center">
              <Text>重学阈值</Text>
              <InputNumber
                min={0}
                max={100}
                precision={0}
                value={
                  Number.isFinite(thresholdDraft.relearnThreshold)
                    ? thresholdDraft.relearnThreshold
                    : null
                }
                onChange={(value) =>
                  setThresholdDraft((prev) => ({
                    ...prev,
                    relearnThreshold: typeof value === "number" ? value : Number.NaN,
                  }))
                }
              />
              <Text>优秀阈值</Text>
              <InputNumber
                min={0}
                max={100}
                precision={0}
                value={
                  Number.isFinite(thresholdDraft.excellentThreshold)
                    ? thresholdDraft.excellentThreshold
                    : null
                }
                onChange={(value) =>
                  setThresholdDraft((prev) => ({
                    ...prev,
                    excellentThreshold: typeof value === "number" ? value : Number.NaN,
                  }))
                }
              />
              <Button
                type="primary"
                icon={<Save size={15} />}
                loading={progressSaving}
                onClick={() => void handleSaveAdjustmentThresholds()}
              >
                保存设置
              </Button>
              <Button onClick={() => void handleResetAdjustmentThresholds()}>
                恢复默认
              </Button>
            </Space>

            {validateAdjustmentThresholds(thresholdDraft) ? (
              <Alert
                type="info"
                showIcon
                message={`当前阈值：${thresholdPreviewText(normalizeAdjustmentThresholds(thresholdDraft))}`}
                description="阈值设置只保存到当前学习项目；未设置的旧项目默认使用重学 60、优秀 80。"
              />
            ) : (
              <Alert
                type="warning"
                showIcon
                message="重学阈值必须小于优秀阈值，且两个值都必须在0到100之间。"
              />
            )}
          </div>
        </Card>

        <div className="grid gap-4 lg:grid-cols-[380px_1fr]">
          <div className="space-y-4">
            <Card title="学习目标输入">
              <Form
                form={form}
                layout="vertical"
                onValuesChange={markProjectDirty}
                initialValues={{
                  learningAssistantRoot: DEFAULT_ENGINE_ROOT,
                  courseName: FIXED_COURSE_NAME,
                  learningGoal: "系统学习",
                  learningCycle: GOAL_CYCLE_MAP["系统学习"],
                  dailyStudyHours: 1,
                  currentLevel: "基础一般：掌握部分概念，但缺少系统复习",
                  finalGoal: "梳理完整课程知识框架",
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
                  name="courseName"
                  label="课程名称"
                  rules={[{ required: true, message: "课程名称不能为空" }]}
                >
                  <Input readOnly />
                </Form.Item>

                <Form.Item
                  name="learningGoal"
                  label="学习目标"
                  rules={[{ required: true, message: "请选择学习目标" }]}
                >
                  <Select
                    options={LEARNING_GOAL_OPTIONS.map((value) => ({ label: value, value }))}
                    onChange={(value) => {
                      form.setFieldsValue({
                        learningGoal: value,
                        learningCycle: GOAL_CYCLE_MAP[value],
                      });
                    }}
                  />
                </Form.Item>

                <Form.Item
                  name="learningCycle"
                  hidden
                  rules={[{ required: true, message: "学习周期不能为空" }]}
                >
                  <Input />
                </Form.Item>

                <div
                  className="mb-6 rounded-md border border-blue-100 bg-blue-50 px-3 py-3"
                  aria-label="推荐学习周期"
                >
                  <div className="mb-2">
                    <Tag color="blue" icon={<Lock size={12} />}>
                      自动推荐
                    </Tag>
                  </div>
                  <div className="mb-1">
                    <Text strong>推荐学习周期：</Text>
                    <Text strong style={{ fontSize: 16 }}>
                      {selectedLearningCycle}
                    </Text>
                  </div>
                  <Text type="secondary">系统根据学习目标自动匹配，不支持手动修改</Text>
                </div>

                <Form.Item
                  name="dailyStudyHours"
                  label="每日可投入时间"
                  rules={[{ required: true, message: "请选择每日可投入时间" }]}
                >
                  <DailyTimeWheelPicker />
                </Form.Item>

                <Form.Item
                  name="currentLevel"
                  label="当前基础"
                  rules={[{ required: true, message: "请选择当前基础" }]}
                >
                  <Select
                    options={CURRENT_LEVEL_OPTIONS.map((value) => ({ label: value, value }))}
                  />
                </Form.Item>

                <Form.Item
                  name="finalGoal"
                  label="最终目标"
                  rules={[{ required: true, message: "请选择最终目标" }]}
                >
                  <Select
                    options={FINAL_GOAL_OPTIONS.map((value) => ({ label: value, value }))}
                  />
                </Form.Item>

                {selectedFinalGoal === CUSTOM_FINAL_GOAL ? (
                  <Form.Item
                    name="finalGoalCustom"
                    label="自定义目标"
                    rules={[{ required: true, message: "请填写自定义目标" }]}
                  >
                    <Input.TextArea
                      autoSize={{ minRows: 2, maxRows: 4 }}
                      placeholder="请填写希望最终达到的具体学习目标"
                    />
                  </Form.Item>
                ) : null}

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
                    icon={<Settings size={15} />}
                    loading={apiConfigLoading}
                    onClick={openAiConfigModal}
                  >
                    配置理解 API
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

            <Card title={`计划数据来源（已选择 ${selectedDocumentSourceIds.length}）`}>
              {!isTauriRuntime() ? (
                <Alert type="info" showIcon message="本地文档数据仅在 Tauri 桌面端可用" />
              ) : (
                <div className="space-y-3">
                  <Space wrap>
                    <Button
                      size="small"
                      onClick={() =>
                        setSelectedDocumentSourceIds(
                          documentFiles
                            .filter(
                              (source) =>
                                source.canUseAsLearningSource &&
                                source.documentSourceId !== null,
                            )
                            .map((source) => source.documentSourceId as number),
                        )
                      }
                    >
                      全选
                    </Button>
                    <Button size="small" onClick={() => setSelectedDocumentSourceIds([])}>
                      清空
                    </Button>
                    <Button
                      size="small"
                      loading={documentTreeLoading}
                      onClick={() => void loadDocumentSources(true)}
                    >
                      刷新
                    </Button>
                  </Space>
                  <Tree
                    blockNode
                    selectable={false}
                    defaultExpandAll={false}
                    treeData={learningSourceTreeData}
                    style={{ background: "transparent" }}
                  />
                </div>
              )}
            </Card>

            <Modal
              title="配置 AI 助学理解 API"
              open={apiConfigOpen}
              confirmLoading={apiConfigSaving}
              okText="保存配置"
              cancelText="取消"
              onOk={handleSaveAiConfig}
              onCancel={() => setApiConfigOpen(false)}
              destroyOnHidden
            >
              <div className="space-y-3">
                <Alert
                  type={apiConfigStatus?.hasApiKey ? "success" : "info"}
                  showIcon
                  description={formatApiConfigStatus(apiConfigStatus)}
                  message={
                    apiConfigStatus?.hasApiKey
                      ? `当前已配置：${apiConfigStatus.model}（来源：${apiConfigStatus.source}）`
                      : "当前未配置 API Key，AI 理解目标会使用模板 fallback。"
                  }
                />
                <Form
                  form={apiForm}
                  layout="vertical"
                  initialValues={{
                    apiBase: "https://api.deepseek.com",
                    model: "deepseek-chat",
                    apiKey: "",
                  }}
                >
                  <Form.Item
                    name="apiBase"
                    label="API Base"
                    rules={[{ required: true, message: "请输入 API Base" }]}
                  >
                    <Input placeholder="例如：https://api.deepseek.com" />
                  </Form.Item>
                  <Form.Item
                    name="model"
                    label="模型"
                    rules={[{ required: true, message: "请输入模型名称" }]}
                  >
                    <Input placeholder="例如：deepseek-chat" />
                  </Form.Item>
                  <Form.Item
                    name="apiKey"
                    label="API Key"
                    rules={[{ required: true, message: "请输入 API Key" }]}
                  >
                    <Input.Password placeholder="只保存在后端本次运行配置中，不会写入前端代码" />
                  </Form.Item>
                </Form>
                <Button danger onClick={handleClearAiConfig} loading={apiConfigSaving}>
                  清除用户配置
                </Button>
              </div>
            </Modal>

            <Modal
              title="学习记录"
              open={progressModalOpen}
              footer={null}
              onCancel={() => setProgressModalOpen(false)}
            >
              <div className="space-y-4">
                <Alert
                  type={progressSaveError ? "warning" : progressRecord ? "success" : "info"}
                  showIcon
                  message={
                    progressSaveError
                      ? "学习记录存在异常"
                      : progressRecord
                        ? "已恢复最近一次学习记录"
                        : "暂无已保存的学习记录"
                  }
                  description={
                    progressSaveError ||
                    (lastSavedAt ? `最近保存时间：${lastSavedAt}` : "生成学习计划后会自动保存。")
                  }
                />

                {progressRecord ? (
                  <Descriptions column={1} size="small" bordered>
                    <Descriptions.Item label="课程">
                      {progressRecord.goal.course}
                    </Descriptions.Item>
                    <Descriptions.Item label="学习目标">
                      {progressRecord.goal.learningGoal}
                    </Descriptions.Item>
                    <Descriptions.Item label="当前阶段">
                      阶段 {progressRecord.currentStageIndex + 1}
                    </Descriptions.Item>
                    <Descriptions.Item label="计划来源">
                      {formatGenerationSource(progressRecord.planSource)}
                    </Descriptions.Item>
                    <Descriptions.Item label="测试记录">
                      {progressRecord.testRecords.length
                        ? `${progressRecord.testRecords.length} 次测试，最近得分 ${
                            progressRecord.testRecords[progressRecord.testRecords.length - 1]?.score
                          }/${
                            progressRecord.testRecords[progressRecord.testRecords.length - 1]
                              ?.maxScore
                          }`
                        : "暂无测试记录"}
                    </Descriptions.Item>
                    <Descriptions.Item label="薄弱点">
                      {progressRecord.testRecords[progressRecord.testRecords.length - 1]
                        ?.weakPoints?.length
                        ? progressRecord.testRecords[
                            progressRecord.testRecords.length - 1
                          ]?.weakPoints.join("、")
                        : "暂无"}
                    </Descriptions.Item>
                  </Descriptions>
                ) : null}

                <Space wrap>
                  <Button
                    icon={<Save size={15} />}
                    loading={progressSaving}
                    disabled={!result}
                    onClick={() => saveProgressRecord(buildProgressRecord())}
                  >
                    立即保存
                  </Button>
                  <Button
                    icon={<RotateCcw size={15} />}
                    disabled={!progressRecord}
                    onClick={() => {
                      if (progressRecord) {
                        restoreProgress(progressRecord);
                        message.success("已恢复最近一次学习记录");
                      }
                    }}
                  >
                    恢复上次计划
                  </Button>
                  <Button onClick={handleStartNewPlan}>开始新计划</Button>
                  <Button danger icon={<Trash2 size={15} />} onClick={handleClearProgress}>
                    清除当前学习记录
                  </Button>
                </Space>
              </div>
            </Modal>

            <Modal
              title="学习项目管理"
              open={projectManagerOpen}
              footer={null}
              width={860}
              onCancel={() => setProjectManagerOpen(false)}
            >
              <div className="space-y-4">
                <Alert
                  type={projectList.length ? "info" : "warning"}
                  showIcon
                  message={projectList.length ? "选择一个学习项目继续" : "还没有学习项目"}
                  description={
                    projectList.length
                      ? "切换项目会先检查当前页面是否有未保存修改。旧版最近一次学习记录会自动迁移为一个独立项目。"
                      : "点击“新建学习项目”后再生成计划、测试和动态调整，后续都会保存到该项目中。"
                  }
                />

                <Space wrap>
                  <Button icon={<PenLine size={15} />} onClick={handleCreateProject}>
                    新建学习项目
                  </Button>
                  <Button
                    icon={<Save size={15} />}
                    loading={progressSaving}
                    onClick={handleManualSaveProject}
                  >
                    保存当前项目
                  </Button>
                  <Button loading={progressLoading} onClick={() => void refreshProjectList()}>
                    刷新列表
                  </Button>
                </Space>

                <List
                  bordered
                  loading={progressLoading}
                  locale={{ emptyText: "暂无学习项目" }}
                  dataSource={projectList}
                  renderItem={(project) => (
                    <List.Item
                      actions={[
                        <Button
                          key="open"
                          size="small"
                          type={currentProjectId === project.projectId ? "primary" : "default"}
                          onClick={() => handleOpenProject(project.projectId)}
                        >
                          {currentProjectId === project.projectId ? "当前项目" : "打开"}
                        </Button>,
                        <Button
                          key="rename"
                          size="small"
                          icon={<PenLine size={13} />}
                          onClick={() => handleRenameProject(project)}
                        >
                          重命名
                        </Button>,
                        <Button
                          key="duplicate"
                          size="small"
                          onClick={() => handleDuplicateProject(project)}
                        >
                          复制
                        </Button>,
                        <Button
                          key="delete"
                          size="small"
                          danger
                          icon={<Trash2 size={13} />}
                          onClick={() => handleDeleteProject(project)}
                        >
                          删除
                        </Button>,
                      ]}
                    >
                      <List.Item.Meta
                        title={
                          <Space wrap>
                            <Text strong>{project.projectName}</Text>
                            {currentProjectId === project.projectId ? (
                              <Tag color="blue">当前</Tag>
                            ) : null}
                            <Tag>{project.courseName || FIXED_COURSE_NAME}</Tag>
                          </Space>
                        }
                        description={
                          <div className="space-y-1">
                            <div>
                              <Text type="secondary">
                                目标：{project.learningGoal || "未填写"}；当前阶段：
                                {project.currentStage || "未生成计划"}；进度：
                                {project.progressPercent}%
                              </Text>
                            </div>
                            <div>
                              <Text type="secondary">
                                最近更新：{project.updatedAt}；上次打开：
                                {project.lastOpenedAt || "暂无"}
                              </Text>
                            </div>
                          </div>
                        }
                      />
                    </List.Item>
                  )}
                />
              </div>
            </Modal>

            <Modal
              title="当前学习项目存在未保存修改"
              open={unsavedConfirmOpen}
              onCancel={() => resolveUnsavedConfirm(false)}
              footer={[
                <Button key="cancel" onClick={() => resolveUnsavedConfirm(false)}>
                  取消
                </Button>,
                <Button key="discard" danger onClick={handleUnsavedDiscardAndContinue}>
                  放弃修改并继续
                </Button>,
                <Button
                  key="save"
                  type="primary"
                  loading={progressSaving}
                  onClick={handleUnsavedSaveAndContinue}
                >
                  保存后继续
                </Button>,
              ]}
            >
              <Alert
                type="warning"
                showIcon
                message="切换、删除或关闭前建议先保存"
                description="如果选择放弃修改，当前页面尚未保存到学习项目中的计划、测试或调整结果会丢失。"
              />
            </Modal>

            <Modal
              title="本阶段建议重新学习"
              open={Boolean(localAdjustmentPrompt)}
              width={760}
              onCancel={() => {
                if (localAdjustmentPrompt) {
                  void handleDeclineLocalRelearnAdjustment(localAdjustmentPrompt.stageIndex);
                }
              }}
              footer={[
                <Button
                  key="decline"
                  onClick={() => {
                    if (localAdjustmentPrompt) {
                      void handleDeclineLocalRelearnAdjustment(localAdjustmentPrompt.stageIndex);
                    }
                  }}
                >
                  暂不调整
                </Button>,
                <Button
                  key="preview"
                  onClick={() =>
                    setLocalAdjustmentPrompt((prev) =>
                      prev ? { ...prev, previewVisible: !prev.previewVisible } : prev,
                    )
                  }
                >
                  {localAdjustmentPrompt?.previewVisible ? "收起调整内容" : "查看调整内容"}
                </Button>,
                <Button
                  key="accept"
                  type="primary"
                  loading={
                    localAdjustmentPrompt
                      ? Boolean(adjustingStages[localAdjustmentPrompt.stageIndex]) || progressSaving
                      : false
                  }
                  onClick={() => {
                    if (localAdjustmentPrompt) {
                      void handleAcceptLocalRelearnAdjustment(localAdjustmentPrompt.stageIndex);
                    }
                  }}
                >
                  采用调整计划
                </Button>,
              ]}
            >
              {localAdjustmentPrompt ? (
                <div className="space-y-4">
                  <Alert
                    type="warning"
                    showIcon
                    message={`本阶段测试结果低于你设置的重学阈值 ${adjustmentThresholds.relearnThreshold} 分，建议调整当前阶段计划，重新学习薄弱知识点。`}
                    description="是否采用该调整建议由你决定，不采用也不会锁定后续阶段。"
                  />
                  <Descriptions column={1} size="small" bordered>
                    <Descriptions.Item label="本次测试分数">
                      {localAdjustmentPrompt.scoreResult.totalScore}/
                      {localAdjustmentPrompt.scoreResult.maxScore}（
                      {localAdjustmentPrompt.scoreResult.percentage ??
                        getScorePercentage(
                          localAdjustmentPrompt.scoreResult.totalScore,
                          localAdjustmentPrompt.scoreResult.maxScore,
                        )}
                      分）
                    </Descriptions.Item>
                    <Descriptions.Item label="掌握等级">需要重学</Descriptions.Item>
                    <Descriptions.Item label="当前阶段">
                      {localPromptStage?.name ?? `阶段 ${localAdjustmentPrompt.stageIndex + 1}`}
                    </Descriptions.Item>
                    <Descriptions.Item label="薄弱知识点">
                      {localAdjustmentPrompt.scoreResult.weakPoints.length
                        ? localAdjustmentPrompt.scoreResult.weakPoints.join("、")
                        : "暂无明确薄弱点，建议复习本阶段核心知识点"}
                    </Descriptions.Item>
                    <Descriptions.Item label="缺失关键词">
                      {localAdjustmentPrompt.scoreResult.missingKeywords.length
                        ? localAdjustmentPrompt.scoreResult.missingKeywords.join("、")
                        : "暂无"}
                    </Descriptions.Item>
                  </Descriptions>

                  {localAdjustmentPrompt.previewVisible && localPromptPreview ? (
                    <Card size="small" title="本地规则调整预览">
                      <div className="space-y-3">
                        <Alert
                          type="info"
                          showIcon
                          message={localPromptPreview.reason}
                          description="本预览只修改当前阶段，不锁定后续阶段，不调用讯飞星火 API。"
                        />
                        <Descriptions column={1} size="small" bordered>
                          <Descriptions.Item label="调整来源">本地规则</Descriptions.Item>
                          <Descriptions.Item label="建议重新测试">
                            {localPromptPreview.needRetest ? "是，建议补学后重新测试" : "否"}
                          </Descriptions.Item>
                          <Descriptions.Item label="后续阶段访问">
                            不锁定，后续阶段保持可查看、可继续学习
                          </Descriptions.Item>
                          <Descriptions.Item label="新增任务">
                            {localPromptPreview.addedTasks.join("；")}
                          </Descriptions.Item>
                          <Descriptions.Item label="依据">
                            {localPromptWeakPoints.length
                              ? localPromptWeakPoints.join("、")
                              : "本阶段核心知识点"}
                          </Descriptions.Item>
                        </Descriptions>
                      </div>
                    </Card>
                  ) : null}
                </div>
              ) : null}
            </Modal>

            <Modal
              title="上一阶段错题复盘"
              open={Boolean(activeWrongQuestionReviewPrompt)}
              width={860}
              onCancel={() => {
                void handleWrongQuestionReviewLater();
              }}
              footer={[
                <Button key="later" onClick={() => void handleWrongQuestionReviewLater()}>
                  稍后提醒
                </Button>,
                <Button key="dismiss" onClick={() => void handleDismissWrongQuestionReview()}>
                  本阶段不再提醒
                </Button>,
                wrongQuestionReviewExpanded ? (
                  <Button
                    key="complete"
                    type="primary"
                    loading={progressSaving}
                    onClick={() => void handleCompleteWrongQuestionReview()}
                  >
                    标记全部已完成复盘
                  </Button>
                ) : (
                  <Button
                    key="start"
                    type="primary"
                    onClick={() => setWrongQuestionReviewExpanded(true)}
                  >
                    开始复盘
                  </Button>
                ),
              ]}
            >
              {activeWrongQuestionReviewPrompt ? (
                <div className="space-y-4">
                  <Alert
                    type="info"
                    showIcon
                    message="你上一阶段已达到基本掌握，但仍存在部分错题。建议在开始本阶段前先复盘这些错题。"
                    description="这是本地评分结果生成的复盘提醒，不会调用 API，也不会锁定后续阶段。"
                  />
                  <Descriptions column={1} size="small" bordered>
                    <Descriptions.Item label="上一阶段">
                      {activeWrongQuestionReviewPrompt.fromStageName}
                    </Descriptions.Item>
                    <Descriptions.Item label="进入阶段">
                      {activeWrongQuestionReviewPrompt.targetStageName}
                    </Descriptions.Item>
                    <Descriptions.Item label="上一阶段测试分数">
                      {activeWrongQuestionReviewPrompt.score}/
                      {activeWrongQuestionReviewPrompt.maxScore}（
                      {activeWrongQuestionReviewPrompt.percentage} 分）
                    </Descriptions.Item>
                    <Descriptions.Item label="掌握等级">
                      {activeWrongQuestionReviewPrompt.masteryLevel}
                    </Descriptions.Item>
                    <Descriptions.Item label="错题数量">
                      {activeWrongQuestionReviewPrompt.wrongQuestions.length} 题
                    </Descriptions.Item>
                    <Descriptions.Item label="薄弱知识点">
                      {activeWrongQuestionReviewPrompt.weakPoints.length
                        ? activeWrongQuestionReviewPrompt.weakPoints.join("、")
                        : "暂无明确薄弱点"}
                    </Descriptions.Item>
                    <Descriptions.Item label="缺失关键词">
                      {activeWrongQuestionReviewPrompt.missingKeywords.length
                        ? activeWrongQuestionReviewPrompt.missingKeywords.join("、")
                        : "暂无"}
                    </Descriptions.Item>
                  </Descriptions>

                  <List
                    size="small"
                    dataSource={
                      wrongQuestionReviewExpanded
                        ? activeWrongQuestionReviewPrompt.wrongQuestions
                        : activeWrongQuestionReviewPrompt.wrongQuestions.slice(0, 3)
                    }
                    renderItem={(item, index) => {
                      const reviewed =
                        activeWrongQuestionReviewPrompt.reviewedQuestionIds.includes(
                          item.questionId,
                        );
                      return (
                        <List.Item style={{ paddingLeft: 0, paddingRight: 0 }}>
                          <Card size="small" className="w-full">
                            <div className="space-y-2">
                              <Space wrap>
                                <Tag color="processing">错题 {index + 1}</Tag>
                                <Tag>{formatQuestionType(item.questionType)}</Tag>
                                <Tag color="blue">
                                  {item.score} / {item.maxScore} 分
                                </Tag>
                                <Tag color="green">{item.knowledgePoint}</Tag>
                                {reviewed ? <Tag color="purple">已复盘</Tag> : null}
                              </Space>
                              <Text strong>{item.questionText}</Text>
                              {item.options.length ? (
                                <div>
                                  <Text type="secondary">
                                    选项：{item.options.join("；")}
                                  </Text>
                                </div>
                              ) : null}
                              <Descriptions column={1} size="small" bordered>
                                <Descriptions.Item label="你的答案">
                                  {item.userAnswer || "未作答"}
                                </Descriptions.Item>
                                <Descriptions.Item label="正确答案">
                                  {item.standardAnswer || "暂无标准答案"}
                                </Descriptions.Item>
                                {item.answerImage ? (
                                  <Descriptions.Item label="答案图">
                                    <img
                                      src={item.answerImage}
                                      alt={`${item.questionId} 答案图`}
                                      className="max-h-[420px] max-w-full rounded-lg border border-gray-200 object-contain"
                                    />
                                  </Descriptions.Item>
                                ) : null}
                                <Descriptions.Item label="失分原因">
                                  {item.wrongReason}
                                </Descriptions.Item>
                                <Descriptions.Item label="缺失关键词">
                                  {item.missingKeywords.length
                                    ? item.missingKeywords.join("、")
                                    : "暂无"}
                                </Descriptions.Item>
                                <Descriptions.Item label="评分反馈">
                                  {item.feedback}
                                </Descriptions.Item>
                                <Descriptions.Item label="复习建议">
                                  {item.reviewSuggestion}
                                </Descriptions.Item>
                              </Descriptions>
                              {wrongQuestionReviewExpanded && !reviewed ? (
                                <Button
                                  size="small"
                                  onClick={() =>
                                    void handleMarkWrongQuestionReviewed(item.questionId)
                                  }
                                >
                                  标记本题已复盘
                                </Button>
                              ) : null}
                            </div>
                          </Card>
                        </List.Item>
                      );
                    }}
                  />
                  {!wrongQuestionReviewExpanded &&
                  activeWrongQuestionReviewPrompt.wrongQuestions.length > 3 ? (
                    <Text type="secondary">
                      还有 {activeWrongQuestionReviewPrompt.wrongQuestions.length - 3} 道错题，点击“开始复盘”查看完整列表。
                    </Text>
                  ) : null}
                </div>
              ) : null}
            </Modal>

            <MaterialUploader onImported={loadDocumentSources} />

            <Card title="本地内容搜索">
              <div className="space-y-3">
                <Input.Search
                  placeholder="输入要查找的知识点、概念或问题"
                  enterButton="搜索本地内容"
                  value={localKbQuery}
                  loading={localKbState.loading}
                  onChange={(event) => setLocalKbQuery(event.target.value)}
                  onSearch={(value) => handleSearchLocalKb(value)}
                />
                <LocalKbList state={localKbState} />
              </div>
            </Card>

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
                  <Descriptions.Item label="结果来源">
                    <div className="space-y-2">
                      <Tag color={["user", "runtime", "env", "spark"].includes(String(result.understanding.source)) ? "green" : "default"}>
                        {formatGenerationSource(result.understanding.source)}
                      </Tag>
                      {!result.stages.length && result.fallbackReason ? (
                        <Alert
                          type="warning"
                          showIcon
                          message={MODEL_CALL_FAILED_PREFIX + result.fallbackReason}
                        />
                      ) : null}
                    </div>
                  </Descriptions.Item>
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
                <>
                  <Alert
                    className="mb-3"
                    type={["user", "runtime", "env", "spark"].includes(String(result.understanding.source)) ? "success" : "info"}
                    showIcon
                    message={formatGenerationSource(result.understanding.source)}
                  />
                  {result.planStrategy ? (
                    <Alert
                      className="mb-3"
                      type="info"
                      showIcon
                      message="本计划策略"
                      description={result.planStrategy}
                    />
                  ) : null}
                  {result.fallbackReason ? (
                    <Alert
                      className="mb-3"
                      type="warning"
                      showIcon
                      message={MODEL_CALL_FAILED_PREFIX + result.fallbackReason}
                    />
                  ) : null}
                </>
              ) : null}
              {planKbContext ? (
                <Alert
                  className="mb-3"
                  type={planKbContext.results.length ? "success" : "info"}
                  showIcon
                  message={
                    planKbContext.results.length
                      ? `生成计划前已检索到 ${planKbContext.results.length} 条本地知识库内容`
                      : planKbContext.message
                  }
                  description={
                    planKbContext.results.length
                      ? "已保存在前端 kbContext 中，后续可传入大模型作为计划生成上下文。"
                      : undefined
                  }
                />
              ) : null}
              {result && (selectedReferenceFiles.length || planKbContext) ? (
                <div className="mb-4 grid gap-4 lg:grid-cols-2">
                  <div>
                    <Text strong>本次选择资料</Text>
                    <List
                      className="mt-2"
                      size="small"
                      bordered
                      locale={{ emptyText: "本次未选择资料" }}
                      dataSource={selectedReferenceFiles}
                      renderItem={(item) => {
                        const documentId = item.source.documentSourceId as number;
                        const importance =
                          sourceImportanceLevels[documentId] ?? "normal";
                        return (
                          <List.Item>
                            <Space wrap size="small">
                              <Text>{item.source.name}</Text>
                              <Tag>
                                {(item.source.fileType || "unknown").toUpperCase()}
                              </Tag>
                              <Tag>{item.folderName}</Tag>
                              <Tag color={importance === "reference" ? "default" : "blue"}>
                                {SOURCE_IMPORTANCE_OPTIONS.find(
                                  (option) => option.value === importance,
                                )?.label ?? importance}
                              </Tag>
                            </Space>
                          </List.Item>
                        );
                      }}
                    />
                  </div>
                  <div>
                    <Text strong>本次实际引用资料</Text>
                    <List
                      className="mt-2"
                      size="small"
                      bordered
                      locale={{ emptyText: "本次没有资料进入计划生成上下文" }}
                      dataSource={actualReferenceFiles}
                      renderItem={(item) => (
                        <List.Item>
                          <Space wrap size="small">
                            <Text>{item.sourceFile}</Text>
                            <Tag>{item.fileType.toUpperCase()}</Tag>
                            <Tag>{item.sourceFolder || "未分类"}</Tag>
                            <Tag color="green">
                              权重 {item.weight} · {item.chunkCount} 个片段
                            </Tag>
                          </Space>
                        </List.Item>
                      )}
                    />
                  </div>
                </div>
              ) : null}
              {result?.localAllocation ? (
                <div className="mb-4 space-y-3">
                  {result.localAllocation.warnings.map((warning) => <Alert key={warning} type="warning" showIcon message={warning} />)}
                  <Card size="small" title="本地学习时间汇总">
                    <Descriptions bordered size="small" column={2}>
                      <Descriptions.Item label="课程完整基准">{result.localAllocation.timeSummary.baselineCourseHours} 小时</Descriptions.Item>
                      <Descriptions.Item label="目标建议时长">{result.localAllocation.timeSummary.targetHours} 小时</Descriptions.Item>
                      <Descriptions.Item label="用户可用时长">{result.localAllocation.timeSummary.availableHours} 小时</Descriptions.Item>
                      <Descriptions.Item label="实际计划时长">{result.localAllocation.timeSummary.plannedHours} 小时</Descriptions.Item>
                      <Descriptions.Item label="时间缺口">{result.localAllocation.timeSummary.missingHours} 小时</Descriptions.Item>
                      <Descriptions.Item label="额外可用时间">{result.localAllocation.timeSummary.extraAvailableHours} 小时</Descriptions.Item>
                      <Descriptions.Item label="目标覆盖率">{(result.localAllocation.timeSummary.targetCoverageRate * 100).toFixed(2)}%</Descriptions.Item>
                      <Descriptions.Item label="推荐每日时间">{formatStudyHours(result.localAllocation.timeSummary.recommendedDailyHours)}</Descriptions.Item>
                      <Descriptions.Item label="用户每日时间">{formatStudyHours(result.localAllocation.timeSummary.dailyStudyHours)}</Descriptions.Item>
                      <Descriptions.Item label="学习周期">{result.localAllocation.timeSummary.totalDays} 天</Descriptions.Item>
                    </Descriptions>
                  </Card>
                  <Card size="small" title="阶段小时分配">
                    <List size="small" dataSource={result.localAllocation.stageAllocations} renderItem={(item) => <List.Item><Text>{item.stageName}</Text><Text>{(item.ratio * 100).toFixed(0)}% · {item.allocatedHours} 小时</Text></List.Item>} />
                  </Card>
                  <Card size="small" title="文件小时分配">
                    <List size="small" dataSource={result.localAllocation.sourceAllocations} renderItem={(item) => <List.Item><Space><Text>{item.displayName}</Text><Tag>{item.category}</Tag><Tag>{SOURCE_IMPORTANCE_OPTIONS.find((option) => option.value === item.importanceLevel)?.label ?? item.importanceLevel}</Tag></Space><Text>{item.includedInPlan ? `${item.allocatedHours} 小时（权重 ${item.importanceWeight}）` : "0 小时，仅供查询"}</Text></List.Item>} />
                  </Card>
                  <Card size="small" title="每日容量分配">
                    <List size="small" dataSource={result.localAllocation.dailyAllocations} renderItem={(item) => <List.Item><Text>第 {item.dayIndex} 天</Text><Text>计划 {item.plannedHours} 小时，剩余 {item.remainingCapacityHours} 小时；{item.stageAllocations.map((stage) => `${stage.stageKey} ${stage.hours}小时`).join("、") || "可选拓展"}</Text></List.Item>} />
                  </Card>
                </div>
              ) : null}
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

                        <LearningEntriesList entries={stage.learningEntries} />

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
                            icon={<SearchCheck size={15} />}
                            loading={stageKbStates[index]?.loading}
                            onClick={() => handleSearchStageKb(stage, index)}
                          >
                            查询本地资料
                          </Button>
                          <Button
                            icon={<PenLine size={15} />}
                            loading={stageQuizzes[index]?.loading}
                            onClick={() => handleStartQuiz(stage, index)}
                          >
                            开始测试
                          </Button>
                          {getWrongQuestionReviewPromptForStage(index) ? (
                            <Button
                              onClick={() => {
                                const prompt = getWrongQuestionReviewPromptForStage(index);
                                if (prompt) {
                                  void openWrongQuestionReviewPrompt(prompt, {
                                    expanded: true,
                                  });
                                }
                              }}
                            >
                              错题复盘
                            </Button>
                          ) : null}
                          <Button
                            icon={<RotateCcw size={15} />}
                            loading={adjustingStages[index]}
                            disabled={adjustingStages[index]}
                            onClick={() => handleAdjustPlan(index)}
                          >
                            调整计划
                          </Button>
                        </Space>

                        <StageKbList state={stageKbStates[index]} />
                        <ResourceList state={stageResources[index]} />
                        <QuizPanel
                          state={stageQuizzes[index]}
                          onAnswerChange={(questionId, answer) =>
                            handleQuizAnswerChange(index, questionId, answer)
                          }
                          onSubmit={() => handleSubmitQuiz(index)}
                        />
                        <AdjustmentResultView
                          result={adjustResults[index]}
                          scoreResult={stageQuizzes[index]?.scoreResult}
                          onUndo={() => handleUndoLastAdjustment(index)}
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
              message="当前 MVP 已能保存并恢复学习计划"
              description="本版已实现目标解析、计划生成、阶段任务展示、阶段测试和最近一次学习记录持久化；正式资源库和动态计划调整仍可继续扩展。"
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
