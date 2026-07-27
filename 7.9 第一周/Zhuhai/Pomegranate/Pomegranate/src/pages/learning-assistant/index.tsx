import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Divider,
  Empty,
  Form,
  Input,
  List,
  Modal,
  Popconfirm,
  Progress,
  Radio,
  Select,
  Space,
  Spin,
  Tag,
  Timeline,
  Typography,
  message,
} from "antd";
import {
  BookOpen,
  CheckCircle2,
  ClipboardList,
  Copy,
  FileText,
  FolderOpen,
  GraduationCap,
  Link2,
  PenLine,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  Sparkles,
  Trash2,
  Upload,
} from "lucide-react";
import {
  createAccountLearningProject,
  deleteAccountLearningProject,
  duplicateAccountLearningProject,
  listAccountLearningProjects,
  openAccountLearningProject,
  renameAccountLearningProject,
  updateAccountLearningProject,
} from "@/lib/learning/accountLearningProjects";
import {
  addAccountLearningProjectDocument,
  listAccountLearningProjectDocuments,
  removeAccountLearningProjectDocument,
  updateAccountLearningProjectDocument,
} from "@/lib/learning/accountLearningProjectDocuments";
import {
  type AccountLearningEnvelope,
  type JsonObject,
  type LearningProjectDetail,
  type LearningProjectDocument,
  type LearningProjectDocumentImportance,
  type LearningProjectSummary,
} from "@/lib/learning/accountLearningTypes";
import {
  pickAndUploadLearningMaterial,
  type LearningMaterialUploadResult,
} from "@/lib/learning/accountLearningUpload";
import {
  isAccountUploadedNote,
  noteApi,
  prepareAccountUploadedMaterial,
  type AccountBackedNote,
} from "@/lib/documents/repository";
import DailyTimeWheelPicker from "./components/DailyTimeWheelPicker";
import {
  attachDiagnosisToUnderstanding,
  buildLearningAssistantDiagnosis,
  extractLearningAssistantDiagnosis,
  type LearningAssistantDiagnosis,
} from "@/lib/learning/learningAssistantDiagnostics";
import {
  appendLearningAssistantQaRecordToProgress,
  buildLearningAssistantQaRecord,
  extractLearningAssistantQaRecords,
  LEARNING_ASSISTANT_QA_SOURCE,
  type LearningAssistantKbSearchResult,
  type LearningAssistantQaRecord,
  type LearningAssistantQaSource,
} from "@/lib/learning/learningAssistantQa";
import {
  buildLearningAssistantProgressOverview,
  type LearningAssistantProgressActivity,
  type LearningAssistantProgressOverview,
  type LearningAssistantStageProgress,
} from "@/lib/learning/learningAssistantProgress";
import {
  appendLearningAssistantQuizRecordToProgress,
  buildLearningAssistantQuizRecord,
  buildLearningAssistantLocalReplan,
  buildLearningAssistantStageQuiz,
  extractLearningAssistantMasteryRecords,
  extractLearningAssistantQuizRecords,
  scoreLearningAssistantQuiz,
  type LearningAssistantMasteryRecord,
  type LearningAssistantQuizQuestion,
  type LearningAssistantQuizRecord,
  type LearningAssistantQuizScoreResult,
} from "@/lib/learning/learningAssistantQuiz";
import {
  findLearningAssistantFallbackResources,
  type LearningAssistantFallbackResource,
} from "@/lib/learning/learningAssistantFallbackResources";
import { useAccountStore } from "@/store/account";

const { Paragraph, Text, Title } = Typography;

const FIXED_COURSE_NAME = "机械制造工艺学";
const FALLBACK_SOURCE = "local-fallback";

const GOAL_CYCLE_OPTIONS = [
  { value: "3天", label: "3天冲刺" },
  { value: "2周", label: "2周查漏补缺" },
  { value: "3周", label: "3周系统学习" },
  { value: "4周", label: "4周综合提升" },
];

const CURRENT_LEVEL_OPTIONS = [
  "零基础：基本没有学习过本课程",
  "基础较弱：学习过，但大部分知识点掌握不牢",
  "基础一般：掌握部分概念，但缺少系统复习",
  "基础较好：已掌握主要内容，需要查漏补缺",
];

const FINAL_GOAL_OPTIONS = [
  "掌握课程基础概念",
  "梳理完整课程知识框架",
  "通过期末考试",
  "期末成绩达到80分以上",
  "能够运用知识解决综合问题",
];

interface LearningGoalFormValues {
  name?: string;
  courseName: string;
  learningGoal: string;
  learningCycle: string;
  dailyStudyHours: number;
  currentLevel: string;
  finalGoal: string;
}

interface LearningAssistantCheckResult {
  ok: boolean;
  knowledgePointsPath: string;
  workbookCount: number;
  knowledgePointCount: number;
  errors: string[];
  warnings: string[];
}

interface LearningAssistantUnderstanding {
  summary: string;
  currentGap: string;
  strategy: string;
  closedLoop: string;
  diagnosis?: LearningAssistantDiagnosis;
  masteredKnowledgePoints?: string[];
  pendingKnowledgePoints?: string[];
  weakKnowledgePoints?: string[];
  source?: string;
  [key: string]: unknown;
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
  sourceType: string;
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
  localAllocation?: JsonObject;
  message?: string;
  fallbackReason?: string;
  error: string | null;
}

interface StageQuizSession {
  questions: LearningAssistantQuizQuestion[];
  answers: Record<string, string>;
  scoring: boolean;
  scoreResult: LearningAssistantQuizScoreResult | null;
  message: string;
}

type LearningDocumentParseStatus = "ready" | "unchecked" | "unavailable" | "unsupported";

interface LearningDocumentParseCheck {
  status: LearningDocumentParseStatus;
  checkedAt?: string;
  detail: string;
}

const DEFAULT_VALUES: LearningGoalFormValues = {
  name: "机械制造工艺学学习项目",
  courseName: FIXED_COURSE_NAME,
  learningGoal: "系统学习",
  learningCycle: "3周",
  dailyStudyHours: 1,
  currentLevel: "基础一般：掌握部分概念，但缺少系统复习",
  finalGoal: "梳理完整课程知识框架",
};

export default function LearningAssistantPage() {
  const currentUser = useAccountStore((state) => state.currentUser);
  const loginStatus = useAccountStore((state) => state.loginStatus);
  const beginLogin = useAccountStore((state) => state.beginLogin);
  const [form] = Form.useForm<LearningGoalFormValues>();

  const [projects, setProjects] = useState<LearningProjectSummary[]>([]);
  const [currentProject, setCurrentProject] = useState<LearningProjectDetail | null>(null);
  const [documents, setDocuments] = useState<LearningProjectDocument[]>([]);
  const [plan, setPlan] = useState<LearningAssistantPlanResult | null>(null);
  const [checkResult, setCheckResult] = useState<LearningAssistantCheckResult | null>(null);
  const [projectModalOpen, setProjectModalOpen] = useState(false);
  const [loadingProjects, setLoadingProjects] = useState(false);
  const [openingProjectId, setOpeningProjectId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [checking, setChecking] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [documentPickerOpen, setDocumentPickerOpen] = useState(false);
  const [availableDocuments, setAvailableDocuments] = useState<AccountBackedNote[]>([]);
  const [loadingAvailableDocuments, setLoadingAvailableDocuments] = useState(false);
  const [linkingDocumentId, setLinkingDocumentId] = useState<string | null>(null);
  const [documentParseChecks, setDocumentParseChecks] = useState<Record<string, LearningDocumentParseCheck>>({});
  const [qaQuestion, setQaQuestion] = useState("");
  const [askingQuestion, setAskingQuestion] = useState(false);
  const [stageQuizzes, setStageQuizzes] = useState<Record<number, StageQuizSession>>({});

  const linkedDocuments = useMemo(
    () => [...documents].sort((a, b) => a.sortOrder - b.sortOrder || a.createdAt.localeCompare(b.createdAt)),
    [documents],
  );
  const activeDiagnosis = useMemo(
    () =>
      extractLearningAssistantDiagnosis(plan?.understanding) ??
      extractLearningAssistantDiagnosis(currentProject?.understanding),
    [currentProject?.understanding, plan?.understanding],
  );
  const qaRecords = useMemo(
    () => extractLearningAssistantQaRecords(currentProject?.progress),
    [currentProject?.progress],
  );
  const quizRecords = useMemo(
    () => extractLearningAssistantQuizRecords(currentProject?.progress),
    [currentProject?.progress],
  );
  const masteryRecords = useMemo(
    () => extractLearningAssistantMasteryRecords(currentProject?.progress),
    [currentProject?.progress],
  );
  const progressOverview = useMemo(
    () =>
      buildLearningAssistantProgressOverview({
        plan,
        progress: currentProject?.progress,
        linkedDocumentCount: documents.length,
        planAdjustments: currentProject?.planAdjustments,
      }),
    [currentProject?.planAdjustments, currentProject?.progress, documents.length, plan],
  );
  const fallbackResourceRecommendations = useMemo(
    () =>
      findLearningAssistantFallbackResources({
        courseName: readString(currentProject?.learningGoal ?? {}, "courseName", FIXED_COURSE_NAME),
        stageGoal: plan?.understanding?.currentGap || currentProject?.goalSummary || "",
        knowledgePoints: [
          ...(activeDiagnosis?.weakKnowledgePoints ?? []),
          ...(masteryRecords
            .filter((record) => record.masteryLevel === "weak")
            .map((record) => record.knowledgePoint)),
          ...collectPlanKnowledgePoints(plan),
        ],
        currentLevel: readString(currentProject?.learningGoal ?? {}, "currentLevel", DEFAULT_VALUES.currentLevel),
        limit: 6,
      }),
    [activeDiagnosis?.weakKnowledgePoints, currentProject?.goalSummary, currentProject?.learningGoal, masteryRecords, plan],
  );
  const pendingReplanRecords = useMemo(
    () =>
      quizRecords.filter(
        (record) =>
          !record.canAdvance &&
          !hasPlanAdjustment(currentProject?.planAdjustments, `replan-${record.recordKey}`),
    ),
  [currentProject?.planAdjustments, quizRecords],
);
  const documentPickerCandidates = useMemo(
    () =>
      availableDocuments.filter(
        (note) =>
          note.deleted_at === null &&
          !documents.some((document) => document.documentId === note.account_document_id),
      ),
    [availableDocuments, documents],
  );

  useEffect(() => {
    if (!currentUser) {
      setProjects([]);
      setCurrentProject(null);
      setDocuments([]);
      setPlan(null);
      setCheckResult(null);
      setQaQuestion("");
      setStageQuizzes({});
      setDocumentPickerOpen(false);
      setAvailableDocuments([]);
      setDocumentParseChecks({});
      form.setFieldsValue(DEFAULT_VALUES);
      return;
    }
    form.setFieldsValue(DEFAULT_VALUES);
    void refreshProjects();
  }, [currentUser?.platformUserId]);

  async function refreshProjects(openManager = false) {
    if (!currentUser) return;
    setLoadingProjects(true);
    try {
      const envelope = await listAccountLearningProjects({ sort: "recent", limit: 50, offset: 0 });
      const data = unwrapEnvelope(envelope, "项目列表已返回，但账号已切换，请重新打开 AI 助学。");
      if (!data) return;
      setProjects(data.projects);
      if (openManager) setProjectModalOpen(true);
    } catch (error) {
      message.error(formatLearningError(error, "读取学习项目失败"));
    } finally {
      setLoadingProjects(false);
    }
  }

  async function handleCreateProject() {
    if (!currentUser) {
      message.warning("请先登录后再创建学习项目。");
      return;
    }
    try {
      const values = await form.validateFields();
      setSaving(true);
      const diagnosis = buildLearningAssistantDiagnosis(goalSnapshotFromValues(values));
      const understanding = attachDiagnosisToUnderstanding(buildDraftUnderstanding(values), diagnosis);
      const created = await createAccountLearningProject({
        name: (values.name ?? inferProjectName(values)).trim(),
        learningType: "course",
        courseName: values.courseName,
        goalSummary: buildGoalSummary(values),
        learningGoal: buildLearningGoal(values),
        understanding: toJsonObject(understanding),
        currentPlan: {},
        progress: buildProgress("draft", null, 0, diagnosis),
        planAdjustments: [],
      });
      const project = unwrapEnvelope(created, "学习项目已创建，但账号已切换，请重新刷新项目列表。");
      if (!project) return;
      openProjectDetail(project);
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(project)));
      setProjectModalOpen(false);
      message.success("学习项目已创建。");
    } catch (error) {
      message.error(formatLearningError(error, "创建学习项目失败"));
    } finally {
      setSaving(false);
    }
  }

  async function handleOpenProject(projectId: string) {
    setOpeningProjectId(projectId);
    try {
      const opened = await openAccountLearningProject(projectId);
      const project = unwrapEnvelope(opened, "学习项目已返回，但账号已切换，页面不会写入旧账号数据。");
      if (!project) return;
      openProjectDetail(project);
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(project)));
      setProjectModalOpen(false);
      await refreshProjectDocuments(project.id);
    } catch (error) {
      message.error(formatLearningError(error, "打开学习项目失败"));
    } finally {
      setOpeningProjectId(null);
    }
  }

  async function handleSaveProject(nextPlan = plan) {
    if (!currentProject) {
      message.warning("请先创建或打开一个学习项目。");
      return null;
    }
    try {
      const values = await form.validateFields();
      setSaving(true);
      const saved = await updateAccountLearningProject({
        projectId: currentProject.id,
        expectedRevision: currentProject.revision,
        name: (values.name ?? currentProject.name).trim(),
        learningType: "course",
        courseName: values.courseName,
        goalSummary: buildGoalSummary(values),
        learningGoal: buildLearningGoal(values),
        understanding: toJsonObject(nextPlan?.understanding ?? currentProject.understanding),
        currentPlan: nextPlan ? toJsonObject(nextPlan) : toJsonObject(currentProject.currentPlan),
        progress: buildProgress(
          nextPlan ? "planned" : "draft",
          nextPlan,
          documents.length,
          extractLearningAssistantDiagnosis(nextPlan?.understanding ?? currentProject.understanding),
          currentProject.progress,
        ),
      });
      const project = unwrapEnvelope(saved, "学习项目已保存，但账号已切换，请刷新后再继续编辑。");
      if (!project) return null;
      openProjectDetail(project);
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(project)));
      message.success("学习项目已保存。");
      return project;
    } catch (error) {
      message.error(formatLearningError(error, "保存学习项目失败"));
      return null;
    } finally {
      setSaving(false);
    }
  }

  async function handleGeneratePlan() {
    if (!currentProject) {
      message.warning("请先创建或打开学习项目，再生成计划。");
      return;
    }
    try {
      const values = await form.validateFields();
      setGenerating(true);
      const generated = await invoke<LearningAssistantPlanResult>("learning_assistant_generate_plan", {
        input: buildPlanInput(values),
      });
      const diagnosis = buildLearningAssistantDiagnosis(goalSnapshotFromValues(values), generated);
      const enhancedPlan: LearningAssistantPlanResult = {
        ...generated,
        understanding: attachDiagnosisToUnderstanding(generated.understanding, diagnosis),
      };
      if (!generated.success || generated.error) {
        message.warning(generated.error || generated.message || "本地 fallback 未生成可用计划。");
      } else if (generated.fallbackReason || generated.message) {
        message.info(generated.message || generated.fallbackReason);
      }
      setPlan(enhancedPlan);
      setStageQuizzes({});
      await handleSaveProject(enhancedPlan);
    } catch (error) {
      message.error(formatLearningError(error, "生成本地 fallback 学习计划失败"));
    } finally {
      setGenerating(false);
    }
  }

  async function handleCheckResources() {
    setChecking(true);
    try {
      const checked = await invoke<LearningAssistantCheckResult>("learning_assistant_check", {
        input: { learningAssistantRoot: null },
      });
      setCheckResult(checked);
      if (checked.ok) {
        message.success("本地知识点资源可用。");
      } else {
        message.warning("本地知识点资源存在问题，请查看检查结果。");
      }
    } catch (error) {
      message.error(formatLearningError(error, "检查本地知识点资源失败"));
    } finally {
      setChecking(false);
    }
  }

  async function handleRenameProject(project: LearningProjectSummary) {
    const name = window.prompt("请输入新的学习项目名称", project.name)?.trim();
    if (!name) return;
    try {
      const renamed = await renameAccountLearningProject({
        projectId: project.id,
        expectedRevision: project.revision,
        name,
      });
      const next = unwrapEnvelope(renamed, "项目已重命名，但账号已切换，请刷新项目列表。");
      if (!next) return;
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(next)));
      if (currentProject?.id === next.id) openProjectDetail(next);
      message.success("项目已重命名。");
    } catch (error) {
      message.error(formatLearningError(error, "重命名学习项目失败"));
    }
  }

  async function handleDuplicateProject(project: LearningProjectSummary) {
    const name = window.prompt("请输入副本项目名称", `${project.name} 副本`)?.trim();
    if (!name) return;
    try {
      const duplicated = await duplicateAccountLearningProject({ projectId: project.id, name });
      const next = unwrapEnvelope(duplicated, "项目已复制，但账号已切换，请刷新项目列表。");
      if (!next) return;
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(next)));
      message.success("项目副本已创建。");
    } catch (error) {
      message.error(formatLearningError(error, "复制学习项目失败"));
    }
  }

  async function handleDeleteProject(project: LearningProjectSummary) {
    try {
      const deleted = await deleteAccountLearningProject({
        projectId: project.id,
        expectedRevision: project.revision,
      });
      const data = unwrapEnvelope(deleted, "项目已删除，但账号已切换，请刷新项目列表。");
      if (!data) return;
      setProjects((previous) => previous.filter((item) => item.id !== data.projectId));
      if (currentProject?.id === data.projectId) {
        setCurrentProject(null);
        setDocuments([]);
        setPlan(null);
        setQaQuestion("");
        setStageQuizzes({});
        setDocumentPickerOpen(false);
        setAvailableDocuments([]);
        setDocumentParseChecks({});
        form.setFieldsValue(DEFAULT_VALUES);
      }
      message.success("学习项目已删除。");
    } catch (error) {
      message.error(formatLearningError(error, "删除学习项目失败"));
    }
  }

  async function handleUploadAndLinkMaterial() {
    if (!currentProject) {
      message.warning("请先创建或打开学习项目，再上传资料。");
      return;
    }
    setUploading(true);
    try {
      const upload = await pickAndUploadLearningMaterial();
      if (!canUseUploadResult(upload)) return;
      const linked = await addAccountLearningProjectDocument({
        projectId: currentProject.id,
        expectedRevision: currentProject.revision,
        documentId: upload.documentId,
        role: "material",
        importance: "normal",
      });
      const data = unwrapEnvelope(linked, "资料已关联，但账号已切换，请重新打开项目确认。");
      if (!data) return;
      setCurrentProject((previous) =>
        previous ? { ...previous, revision: data.projectRevision } : previous,
      );
      setDocuments((previous) => upsertDocument(previous, data.document));
      message.success("资料已上传并关联到当前学习项目。");
    } catch (error) {
      message.error(formatLearningError(error, "上传或关联资料失败"));
    } finally {
      setUploading(false);
    }
  }

  async function handleLoadAvailableDocuments(openPicker = true) {
    if (!currentProject) {
      message.warning("请先创建或打开学习项目，再选择已有账号文档。");
      return;
    }
    setLoadingAvailableDocuments(true);
    try {
      const notes = await listAccountBackedNotes();
      setAvailableDocuments(notes);
      if (openPicker) setDocumentPickerOpen(true);
    } catch (error) {
      message.error(formatLearningError(error, "读取账号文档失败"));
    } finally {
      setLoadingAvailableDocuments(false);
    }
  }

  async function handleLinkExistingDocument(note: AccountBackedNote) {
    if (!currentProject) return;
    if (documents.some((document) => document.documentId === note.account_document_id)) {
      message.info("该文档已经关联到当前学习项目。");
      return;
    }
    setLinkingDocumentId(note.account_document_id);
    try {
      const linked = await addAccountLearningProjectDocument({
        projectId: currentProject.id,
        expectedRevision: currentProject.revision,
        documentId: note.account_document_id,
        role: "material",
        importance: "normal",
      });
      const data = unwrapEnvelope(linked, "资料已关联，但账号已切换，请重新打开项目确认。");
      if (!data) return;
      setCurrentProject((previous) =>
        previous ? { ...previous, revision: data.projectRevision } : previous,
      );
      setDocuments((previous) => upsertDocument(previous, data.document));
      setDocumentPickerOpen(false);
      message.success("已有账号文档已关联到当前学习项目。");
    } catch (error) {
      message.error(formatLearningError(error, "关联已有账号文档失败"));
    } finally {
      setLinkingDocumentId(null);
    }
  }

  async function handleCheckDocumentParseStatus(documentId: string) {
    setDocumentParseChecks((previous) => ({
      ...previous,
      [documentId]: {
        status: "unchecked",
        detail: "正在检查解析可用性...",
      },
    }));
    try {
      const note = await findAccountNoteByDocumentId(documentId, availableDocuments);
      if (!note) {
        setDocumentParseChecks((previous) => ({
          ...previous,
          [documentId]: {
            status: "unavailable",
            checkedAt: new Date().toISOString(),
            detail: "未在当前账号文档列表中找到该资料，可能已删除或账号已切换。",
          },
        }));
        return;
      }
      if (!isAccountUploadedNote(note)) {
        setDocumentParseChecks((previous) => ({
          ...previous,
          [documentId]: {
            status: "ready",
            checkedAt: new Date().toISOString(),
            detail: "Markdown 文档可直接作为学习资料来源。",
          },
        }));
        return;
      }
      const content = await prepareAccountUploadedMaterial(note);
      setDocumentParseChecks((previous) => ({
        ...previous,
        [documentId]: {
          status: content.trim() ? "ready" : "unsupported",
          checkedAt: new Date().toISOString(),
          detail: content.trim()
            ? `解析可用，已读取约 ${content.trim().length} 个字符。`
            : "文件存在，但当前解析器没有读取到可用正文。",
        },
      }));
    } catch (error) {
      setDocumentParseChecks((previous) => ({
        ...previous,
        [documentId]: {
          status: "unavailable",
          checkedAt: new Date().toISOString(),
          detail: formatLearningError(error, "解析状态检查失败"),
        },
      }));
    }
  }

  async function handleDocumentImportanceChange(
    document: LearningProjectDocument,
    importance: LearningProjectDocumentImportance,
  ) {
    if (!currentProject) return;
    try {
      const updated = await updateAccountLearningProjectDocument({
        projectId: currentProject.id,
        documentId: document.documentId,
        expectedRevision: currentProject.revision,
        importance,
      });
      const data = unwrapEnvelope(updated, "资料重要度已更新，但账号已切换，请刷新项目。");
      if (!data) return;
      setCurrentProject((previous) =>
        previous ? { ...previous, revision: data.projectRevision } : previous,
      );
      setDocuments((previous) => upsertDocument(previous, data.document));
    } catch (error) {
      message.error(formatLearningError(error, "更新资料重要度失败"));
    }
  }

  async function handleRemoveDocument(document: LearningProjectDocument) {
    if (!currentProject) return;
    try {
      const removed = await removeAccountLearningProjectDocument({
        projectId: currentProject.id,
        documentId: document.documentId,
        expectedRevision: currentProject.revision,
      });
      const data = unwrapEnvelope(removed, "资料关联已移除，但账号已切换，请刷新项目。");
      if (!data) return;
      setCurrentProject((previous) =>
        previous ? { ...previous, revision: data.projectRevision } : previous,
      );
      setDocuments((previous) => previous.filter((item) => item.documentId !== document.documentId));
      message.success("资料关联已移除，原账号文档未删除。");
    } catch (error) {
      message.error(formatLearningError(error, "移除资料关联失败"));
    }
  }

  async function refreshProjectDocuments(projectId = currentProject?.id) {
    if (!projectId) return;
    try {
      const envelope = await listAccountLearningProjectDocuments(projectId);
      const data = unwrapEnvelope(envelope, "资料列表已返回，但账号已切换，页面不会写入旧账号数据。");
      if (!data) return;
      setDocuments(data.documents);
      setCurrentProject((previous) =>
        previous && previous.id === projectId
          ? { ...previous, revision: data.projectRevision }
          : previous,
      );
    } catch (error) {
      message.error(formatLearningError(error, "读取项目资料失败"));
    }
  }

  async function handleAskKnowledgeQuestion(searchText = qaQuestion) {
    if (!currentProject) {
      message.warning("请先创建或打开学习项目，再使用知识库问答。");
      return;
    }
    const question = searchText.trim();
    if (!question) {
      message.warning("请输入要询问的内容。");
      return;
    }
    setQaQuestion(question);
    setAskingQuestion(true);
    try {
      const values = form.getFieldsValue();
      const searched = await invoke<LearningAssistantKbSearchResult>("learning_kb_search", {
        input: buildKnowledgeQuestionInput(values, plan, question),
      });
      const record = buildLearningAssistantQaRecord({
        question,
        searched,
        documents: linkedDocuments,
      });
      const saved = await updateAccountLearningProject({
        projectId: currentProject.id,
        expectedRevision: currentProject.revision,
        progress: appendLearningAssistantQaRecordToProgress(currentProject.progress, record),
      });
      const project = unwrapEnvelope(saved, "问答记录已保存，但账号已切换，请重新打开项目确认。");
      if (!project) return;
      openProjectDetail(project);
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(project)));
      setQaQuestion("");
      message.success(
        record.generationType === LEARNING_ASSISTANT_QA_SOURCE
          ? "知识库回答已生成并保存到当前项目。"
          : "本地知识库暂无命中，已保存本次提问记录。",
      );
    } catch (error) {
      message.error(formatLearningError(error, "知识库问答失败"));
    } finally {
      setAskingQuestion(false);
    }
  }

  function handleStartStageQuiz(stage: LearningAssistantStage, stageIndex: number) {
    if (!currentProject) {
      message.warning("请先创建或打开学习项目，再生成阶段测试。");
      return;
    }
    const values = form.getFieldsValue();
    const questions = buildLearningAssistantStageQuiz({
      stage,
      stageIndex,
      currentLevel: values.currentLevel,
      limit: 5,
    });
    setStageQuizzes((previous) => ({
      ...previous,
      [stageIndex]: {
        questions,
        answers: {},
        scoring: false,
        scoreResult: null,
        message: questions.length
          ? "本阶段测试由当前 fallback 计划和本地知识点条目生成，未调用正式题库。"
          : "当前阶段缺少可用知识点，暂无法生成测试。",
      },
    }));
  }

  function handleQuizAnswerChange(stageIndex: number, questionKey: string, answer: string) {
    setStageQuizzes((previous) => {
      const session = previous[stageIndex];
      if (!session) return previous;
      return {
        ...previous,
        [stageIndex]: {
          ...session,
          answers: {
            ...session.answers,
            [questionKey]: answer,
          },
        },
      };
    });
  }

  async function handleSubmitStageQuiz(stage: LearningAssistantStage, stageIndex: number) {
    if (!currentProject) {
      message.warning("请先创建或打开学习项目，再提交测试。");
      return;
    }
    const session = stageQuizzes[stageIndex];
    if (!session?.questions.length) return;
    setStageQuizzes((previous) => ({
      ...previous,
      [stageIndex]: {
        ...session,
        scoring: true,
      },
    }));
    try {
      const answers = session.questions.map((question) => ({
        questionKey: question.questionKey,
        userAnswer: session.answers[question.questionKey] ?? "",
      }));
      const scoreResult = scoreLearningAssistantQuiz(session.questions, answers);
      const record = buildLearningAssistantQuizRecord({
        stage,
        stageIndex,
        questions: session.questions,
        answers: session.answers,
        scoreResult,
      });
      const progress = appendLearningAssistantQuizRecordToProgress(currentProject.progress, record);
      const hasReplanSuggestion = !scoreResult.canAdvance && Boolean(buildLearningAssistantLocalReplan(plan, record));
      const saved = await updateAccountLearningProject({
        projectId: currentProject.id,
        expectedRevision: currentProject.revision,
        progress,
      });
      const project = unwrapEnvelope(saved, "测试结果已保存，但账号已切换，请重新打开项目确认。");
      if (!project) return;
      openProjectDetail(project);
      setStageQuizzes((previous) => ({
        ...previous,
        [stageIndex]: {
          ...session,
          scoring: false,
          scoreResult,
        },
      }));
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(project)));
      message.success(
        hasReplanSuggestion
          ? "阶段测试结果已保存，已生成待确认的重新规划建议。"
          : "阶段测试结果已保存到当前项目。",
      );
    } catch (error) {
      setStageQuizzes((previous) => ({
        ...previous,
        [stageIndex]: {
          ...session,
          scoring: false,
        },
      }));
      message.error(formatLearningError(error, "提交阶段测试失败"));
    }
  }

  async function handleApplyReplan(record: LearningAssistantQuizRecord) {
    if (!currentProject || !plan) return;
    if (hasPlanAdjustment(currentProject.planAdjustments, `replan-${record.recordKey}`)) {
      message.info("该测试结果对应的调整已经应用。");
      return;
    }
    const replan = buildLearningAssistantLocalReplan(plan, record);
    if (!replan) {
      message.info("当前测试结果不需要重新规划。");
      return;
    }
    setSaving(true);
    try {
      const saved = await updateAccountLearningProject({
        projectId: currentProject.id,
        expectedRevision: currentProject.revision,
        currentPlan: toJsonObject(replan.plan),
        progress: buildProgress("planned", replan.plan, documents.length, activeDiagnosis, currentProject.progress),
        planAdjustments: [
          ...currentProject.planAdjustments,
          toJsonObject(replan.adjustment),
        ],
      });
      const project = unwrapEnvelope(saved, "重新规划已保存，但账号已切换，请重新打开项目确认。");
      if (!project) return;
      openProjectDetail(project);
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(project)));
      message.success("已根据错题和薄弱点追加补学、资料和复测任务。");
    } catch (error) {
      message.error(formatLearningError(error, "应用重新规划失败"));
    } finally {
      setSaving(false);
    }
  }

  async function handleAddFallbackResourceToPlan(resource: LearningAssistantFallbackResource) {
    if (!currentProject || !plan) {
      message.warning("请先打开项目并生成学习计划，再加入推荐资源。");
      return;
    }
    const nextPlan = addFallbackResourceToPlan(plan, resource);
    if (nextPlan === plan) {
      message.info("该推荐资源已在当前计划中。");
      return;
    }
    setSaving(true);
    try {
      const saved = await updateAccountLearningProject({
        projectId: currentProject.id,
        expectedRevision: currentProject.revision,
        currentPlan: toJsonObject(nextPlan),
        progress: buildProgress("planned", nextPlan, documents.length, activeDiagnosis, currentProject.progress),
      });
      const project = unwrapEnvelope(saved, "推荐资源已加入计划，但账号已切换，请重新打开项目确认。");
      if (!project) return;
      openProjectDetail(project);
      setProjects((previous) => upsertProjectSummary(previous, summaryFromDetail(project)));
      message.success("推荐资源已加入当前学习计划。");
    } catch (error) {
      message.error(formatLearningError(error, "加入推荐资源失败"));
    } finally {
      setSaving(false);
    }
  }

  function openProjectDetail(project: LearningProjectDetail) {
    setCurrentProject(project);
    form.setFieldsValue(formValuesFromProject(project));
    setPlan(isLearningPlan(project.currentPlan) ? project.currentPlan : null);
    setStageQuizzes({});
  }

  function unwrapEnvelope<T>(
    envelope: AccountLearningEnvelope<T>,
    changedMessage: string,
  ): T | null {
    if (envelope.status === "accountChanged") {
      message.warning("账号状态已变化，本次操作没有发送到服务器。");
      return null;
    }
    if (envelope.status === "completedAccountChanged") {
      message.warning(changedMessage);
      return null;
    }
    return envelope.data;
  }

  function canUseUploadResult(upload: LearningMaterialUploadResult): upload is Extract<LearningMaterialUploadResult, { status: "uploaded" }> {
    if (upload.status === "cancelled") return false;
    if (upload.status === "accountChanged") {
      message.warning("账号已切换，文件未上传。");
      return false;
    }
    if (upload.status === "uploadedAccountChanged") {
      message.warning("文件已上传到原账号的助学目录，但账号已切换，未关联当前项目。");
      return false;
    }
    return true;
  }

  if (!currentUser) {
    return (
      <div className="h-full overflow-auto bg-slate-50">
        <div className="mx-auto flex min-h-full max-w-3xl items-center justify-center p-6">
          <Card className="w-full text-center">
            <GraduationCap size={42} className="mx-auto mb-3 text-blue-500" />
            <Title level={3}>AI 助学需要登录后使用</Title>
            <Paragraph type="secondary">
              学习项目、上传资料和资料关联都保存在 Account Server，并按当前账号隔离。
              当前版本不提供离线项目查看或编辑。
            </Paragraph>
            <Button type="primary" loading={loginStatus === "waiting"} onClick={() => void beginLogin()}>
              登录账号
            </Button>
          </Card>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto bg-[#f6f8fb]">
      <div className="mx-auto max-w-7xl p-6">
        <div className="mb-5 flex flex-wrap items-start justify-between gap-3">
          <div>
            <Title level={3} className="!mb-1 flex items-center gap-2">
              <GraduationCap size={28} />
              AI 助学
            </Title>
            <Text type="secondary">
              第一版仅开放账号项目、本地 fallback 计划、助学资料上传与文档关联。
            </Text>
          </div>
          <Space wrap>
            <Tag color="blue">在线账号：{currentUser.displayName || currentUser.username}</Tag>
            <Tag color="gold">仅本地 fallback</Tag>
            {currentProject ? <Tag color="green">revision {currentProject.revision}</Tag> : null}
          </Space>
        </div>

        <Alert
          className="mb-5"
          type="info"
          showIcon
          message="当前页面不会调用真实模型 API"
          description="模型配置、题库、插件增强和资源推荐入口暂时隐藏；后续完成账号凭据、题库归属和插件边界后再逐项开放。"
        />

        <div className="grid gap-5 xl:grid-cols-[360px_minmax(0,1fr)]">
          <div className="space-y-5">
            <Card
              title="学习项目"
              extra={
                <Button
                  size="small"
                  icon={<FolderOpen size={14} />}
                  loading={loadingProjects}
                  onClick={() => void refreshProjects(true)}
                >
                  管理
                </Button>
              }
            >
              {currentProject ? (
                <Space direction="vertical" size="small" className="w-full">
                  <Text strong>{currentProject.name}</Text>
                  <Text type="secondary">{currentProject.goalSummary || "尚未填写学习目标"}</Text>
                  <Space wrap size="small">
                    <Tag>{currentProject.courseName || FIXED_COURSE_NAME}</Tag>
                    <Tag>{currentProject.learningType || "course"}</Tag>
                    <Tag>资料 {documents.length}</Tag>
                  </Space>
                </Space>
              ) : (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="尚未打开学习项目" />
              )}
              <Divider className="!my-4" />
              <Space wrap>
                <Button type="primary" icon={<PenLine size={14} />} loading={saving} onClick={handleCreateProject}>
                  新建项目
                </Button>
                <Button icon={<RefreshCw size={14} />} loading={loadingProjects} onClick={() => void refreshProjects(true)}>
                  项目列表
                </Button>
              </Space>
            </Card>

            <Card title="本地资源检查">
              <Space direction="vertical" className="w-full">
                <Button icon={<BookOpen size={14} />} loading={checking} onClick={handleCheckResources}>
                  检查知识点 Excel
                </Button>
                {checkResult ? (
                  <Descriptions size="small" column={1} bordered>
                    <Descriptions.Item label="状态">
                      {checkResult.ok ? <Tag color="green">可用</Tag> : <Tag color="red">异常</Tag>}
                    </Descriptions.Item>
                    <Descriptions.Item label="Excel">{checkResult.workbookCount}</Descriptions.Item>
                    <Descriptions.Item label="知识点">{checkResult.knowledgePointCount}</Descriptions.Item>
                    <Descriptions.Item label="路径">{checkResult.knowledgePointsPath}</Descriptions.Item>
                  </Descriptions>
                ) : (
                  <Text type="secondary">使用打包只读资源，不读取来源开发目录。</Text>
                )}
              </Space>
            </Card>

            <FallbackResourcesPanel
              resources={fallbackResourceRecommendations}
              disabled={!currentProject || !plan || saving}
              onAdd={(resource) => void handleAddFallbackResourceToPlan(resource)}
            />

            <Card title="项目资料">
              {currentProject ? (
                <Space direction="vertical" className="w-full">
                  <Button
                    block
                    type="primary"
                    icon={<Upload size={14} />}
                    loading={uploading}
                    onClick={handleUploadAndLinkMaterial}
                  >
                    上传到助学模块上传并关联
                  </Button>
                  <Button
                    block
                    icon={<Search size={14} />}
                    loading={loadingAvailableDocuments}
                    onClick={() => void handleLoadAvailableDocuments(true)}
                  >
                    选择文档中心资料
                  </Button>
                  <Button block icon={<RefreshCw size={14} />} onClick={() => void refreshProjectDocuments()}>
                    刷新资料关联
                  </Button>
                  <List
                    size="small"
                    dataSource={linkedDocuments}
                    locale={{ emptyText: "当前项目暂无关联资料" }}
                    renderItem={(document) => (
                      <List.Item
                        actions={[
                          <Button
                            key="parse"
                            type="text"
                            size="small"
                            icon={<ClipboardList size={14} />}
                            onClick={() => void handleCheckDocumentParseStatus(document.documentId)}
                          >
                            解析状态
                          </Button>,
                          <Select
                            key="importance"
                            size="small"
                            value={document.importance}
                            style={{ width: 92 }}
                            options={[
                              { value: "normal", label: "普通" },
                              { value: "important", label: "重要" },
                              { value: "core", label: "核心" },
                            ]}
                            onChange={(value) => void handleDocumentImportanceChange(document, value)}
                          />,
                          <Popconfirm
                            key="remove"
                            title="移除资料关联"
                            description="只移除当前项目关联，不删除账号文档。"
                            okText="移除"
                            cancelText="取消"
                            onConfirm={() => void handleRemoveDocument(document)}
                          >
                            <Button type="text" danger icon={<Trash2 size={14} />} />
                          </Popconfirm>,
                        ]}
                      >
                        <List.Item.Meta
                          avatar={<FileText size={18} />}
                          title={
                            <Space size="small" wrap>
                              <Text>{document.title}</Text>
                              {document.status === "deleted" ? <Tag color="red">资料已删除</Tag> : null}
                            </Space>
                          }
                          description={
                            <Space direction="vertical" size={2}>
                              <Space size="small" wrap>
                                <Tag>{document.documentType}</Tag>
                                <Tag>{document.role}</Tag>
                                <Text type="secondary">排序 {document.sortOrder}</Text>
                              </Space>
                              <DocumentParseStatusLine check={documentParseChecks[document.documentId]} />
                            </Space>
                          }
                        />
                      </List.Item>
                    )}
                  />
                </Space>
              ) : (
                <Text type="secondary">打开项目后可上传并关联账号文档。</Text>
              )}
            </Card>
          </div>

          <div className="space-y-5">
            <Card
              title="学习目标"
              extra={
                <Space>
                  <Button icon={<Save size={14} />} loading={saving} disabled={!currentProject} onClick={() => void handleSaveProject()}>
                    保存
                  </Button>
                  <Button
                    type="primary"
                    icon={<Sparkles size={14} />}
                    loading={generating}
                    disabled={!currentProject}
                    onClick={handleGeneratePlan}
                  >
                    生成 fallback 计划
                  </Button>
                </Space>
              }
            >
              <Form form={form} layout="vertical" initialValues={DEFAULT_VALUES}>
                <div className="grid gap-4 md:grid-cols-2">
                  <Form.Item name="name" label="项目名称" rules={[{ required: true, message: "请输入项目名称" }]}>
                    <Input placeholder="例如：机械制造工艺学期末复习" />
                  </Form.Item>
                  <Form.Item name="courseName" label="课程" rules={[{ required: true }]}>
                    <Input disabled />
                  </Form.Item>
                  <Form.Item name="learningGoal" label="学习目标" rules={[{ required: true, message: "请选择学习目标" }]}>
                    <Select
                      options={[
                        { value: "期末冲刺", label: "期末冲刺" },
                        { value: "查漏补缺", label: "查漏补缺" },
                        { value: "系统学习", label: "系统学习" },
                        { value: "综合提升", label: "综合提升" },
                      ]}
                    />
                  </Form.Item>
                  <Form.Item name="learningCycle" label="学习周期" rules={[{ required: true }]}>
                    <Select options={GOAL_CYCLE_OPTIONS} />
                  </Form.Item>
                  <Form.Item
                    name="dailyStudyHours"
                    label="每日投入时间"
                    rules={[{ required: true, message: "请选择每日可投入时间" }]}
                  >
                    <DailyTimeWheelPicker />
                  </Form.Item>
                  <Form.Item name="currentLevel" label="当前基础" rules={[{ required: true }]}>
                    <Select options={CURRENT_LEVEL_OPTIONS.map((value) => ({ value, label: value }))} />
                  </Form.Item>
                </div>
                <Form.Item name="finalGoal" label="最终目标" rules={[{ required: true }]}>
                  <Select options={FINAL_GOAL_OPTIONS.map((value) => ({ value, label: value }))} />
                </Form.Item>
              </Form>
            </Card>

            <Card title="初始学情诊断">
              {activeDiagnosis ? (
                <Space direction="vertical" className="w-full">
                  <Alert
                    type={activeDiagnosis.weakKnowledgePoints.length ? "warning" : "info"}
                    showIcon
                    message={activeDiagnosis.summary}
                    description={`依据：${activeDiagnosis.basis.join("、")}；生成时间：${activeDiagnosis.generatedAt}`}
                  />
                  <DiagnosisTagGroup
                    title="已掌握"
                    color="green"
                    emptyText="暂无明确已掌握知识点"
                    items={activeDiagnosis.masteredKnowledgePoints}
                  />
                  <DiagnosisTagGroup
                    title="待学习"
                    color="blue"
                    emptyText="生成计划后显示知识点"
                    items={activeDiagnosis.pendingKnowledgePoints}
                  />
                  <DiagnosisTagGroup
                    title="薄弱点"
                    color="orange"
                    emptyText="暂无明确薄弱点"
                    items={activeDiagnosis.weakKnowledgePoints}
                  />
                  <TaskGroup title="复习建议" items={activeDiagnosis.suggestions} />
                </Space>
              ) : (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description="创建项目后会保存基础诊断，生成 fallback 计划后会结合知识点更新。"
                />
              )}
            </Card>

            <Card title="学习进度">
              {currentProject ? (
                <LearningProgressOverviewPanel overview={progressOverview} />
              ) : (
                <Text type="secondary">
                  打开项目后会汇总阶段完成情况、测试记录、掌握度、资料和最近学习活动。
                </Text>
              )}
            </Card>

            <Card title="知识库问答与项目记忆">
              {currentProject ? (
                <Space direction="vertical" className="w-full">
                  <Alert
                    type="info"
                    showIcon
                    message="回答仅基于本地知识点 Excel 检索结果"
                    description="关联资料目前作为项目线索展示，第一版不解析资料正文；没有命中时会明确标记为不可用，不会编造引用。"
                  />
                  <Input.Search
                    value={qaQuestion}
                    allowClear
                    enterButton="提问并保存"
                    loading={askingQuestion}
                    placeholder="例如：工艺规程设计的步骤是什么？"
                    onChange={(event) => setQaQuestion(event.target.value)}
                    onSearch={(value) => void handleAskKnowledgeQuestion(value)}
                  />
                  <QaRecordList records={qaRecords} />
                </Space>
              ) : (
                <Text type="secondary">打开项目后可使用本地知识库问答，问答记录会在线保存到当前项目。</Text>
              )}
            </Card>

            <Card title="测试记录与掌握反馈">
              {currentProject ? (
                <Space direction="vertical" className="w-full">
                  <MasteryRecordSummary records={masteryRecords} />
                  <ReplanSuggestionPanel
                    records={pendingReplanRecords}
                    onApply={(record) => void handleApplyReplan(record)}
                    applying={saving}
                  />
                  <QuizRecordList
                    records={quizRecords}
                    appliedRecordKeys={new Set(
                      quizRecords
                        .filter((record) =>
                          hasPlanAdjustment(currentProject.planAdjustments, `replan-${record.recordKey}`),
                        )
                        .map((record) => record.recordKey),
                    )}
                    onRetake={(record) => {
                      const stage = plan?.stages[record.stageIndex];
                      if (!stage) {
                        message.warning("当前计划中找不到对应阶段，请先刷新或重新打开项目。");
                        return;
                      }
                      handleStartStageQuiz(stage, record.stageIndex);
                    }}
                  />
                </Space>
              ) : (
                <Text type="secondary">打开项目后可生成阶段测试，提交后题目、答案、解析和分数会保存到当前项目。</Text>
              )}
            </Card>

            <Card title="学习计划">
              {generating ? (
                <div className="flex min-h-48 items-center justify-center">
                  <Spin tip="正在使用本地 fallback 生成计划..." />
                </div>
              ) : plan ? (
                <div className="space-y-4">
                  <Alert
                    type={plan.success ? "success" : "warning"}
                    showIcon
                    message={plan.message || (plan.success ? "计划已生成" : "计划生成不完整")}
                    description={plan.fallbackReason || plan.error || "结果来自本地 fallback 和只读知识点资源。"}
                  />
                  <Descriptions size="small" column={1} bordered>
                    <Descriptions.Item label="目标理解">{plan.understanding.summary}</Descriptions.Item>
                    <Descriptions.Item label="当前差距">{plan.understanding.currentGap}</Descriptions.Item>
                    <Descriptions.Item label="策略">{plan.understanding.strategy}</Descriptions.Item>
                  </Descriptions>
                  <List
                    dataSource={plan.stages}
                    locale={{ emptyText: "暂无阶段计划" }}
                    renderItem={(stage, index) => (
                      <List.Item>
                        <Card size="small" className="w-full" title={`${index + 1}. ${stage.name}`}>
                          <Space direction="vertical" className="w-full">
                            <Text type="secondary">{stage.timeRange}</Text>
                            <Paragraph>{stage.goal}</Paragraph>
                            <TaskGroup title="学习任务" items={stage.learningTasks} />
                            <TaskGroup title="推荐资料" items={stage.resourceTasks} />
                            <TaskGroup title="练习任务" items={stage.practiceTasks} />
                            <TaskGroup title="检查标准" items={stage.completionCriteria} />
                            <Space wrap>
                              <Button
                                icon={<PenLine size={14} />}
                                onClick={() => handleStartStageQuiz(stage, index)}
                              >
                                生成阶段测试
                              </Button>
                            </Space>
                            <StageQuizPanel
                              session={stageQuizzes[index]}
                              onRetake={() => handleStartStageQuiz(stage, index)}
                              onAnswerChange={(questionKey, answer) =>
                                handleQuizAnswerChange(index, questionKey, answer)
                              }
                              onSubmit={() => void handleSubmitStageQuiz(stage, index)}
                            />
                            {stage.learningEntries?.length ? (
                              <List
                                size="small"
                                dataSource={stage.learningEntries}
                                renderItem={(entry) => (
                                  <List.Item>
                                    <Space direction="vertical" size={0}>
                                      <Text strong>{entry.title}</Text>
                                      <Text type="secondary">
                                        {entry.section} · {entry.estimatedMinutes} 分钟 · {entry.sourceFile || "本地知识点"}
                                      </Text>
                                    </Space>
                                  </List.Item>
                                )}
                              />
                            ) : null}
                          </Space>
                        </Card>
                      </List.Item>
                    )}
                  />
                </div>
              ) : (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description="创建或打开项目后，填写目标并生成本地 fallback 计划。"
                />
              )}
            </Card>
          </div>
        </div>
      </div>

      <Modal
        title="学习项目管理"
        open={projectModalOpen}
        footer={null}
        width={860}
        onCancel={() => setProjectModalOpen(false)}
      >
        <Space className="mb-3" wrap>
          <Button type="primary" icon={<PenLine size={14} />} loading={saving} onClick={handleCreateProject}>
            按当前表单新建
          </Button>
          <Button icon={<RefreshCw size={14} />} loading={loadingProjects} onClick={() => void refreshProjects()}>
            刷新
          </Button>
        </Space>
        <List
          bordered
          loading={loadingProjects}
          dataSource={projects}
          locale={{ emptyText: "暂无学习项目" }}
          renderItem={(project) => (
            <List.Item
              actions={[
                <Button
                  key="open"
                  type={currentProject?.id === project.id ? "primary" : "default"}
                  size="small"
                  loading={openingProjectId === project.id}
                  onClick={() => void handleOpenProject(project.id)}
                >
                  {currentProject?.id === project.id ? "当前项目" : "打开"}
                </Button>,
                <Button key="rename" size="small" icon={<PenLine size={13} />} onClick={() => void handleRenameProject(project)}>
                  重命名
                </Button>,
                <Button key="duplicate" size="small" icon={<Copy size={13} />} onClick={() => void handleDuplicateProject(project)}>
                  复制
                </Button>,
                <Popconfirm
                  key="delete"
                  title="删除学习项目"
                  description="项目会软删除，不会删除账号文档。"
                  okText="删除"
                  cancelText="取消"
                  onConfirm={() => void handleDeleteProject(project)}
                >
                  <Button size="small" danger icon={<Trash2 size={13} />}>
                    删除
                  </Button>
                </Popconfirm>,
              ]}
            >
              <List.Item.Meta
                avatar={<Link2 size={18} />}
                title={<Text strong>{project.name}</Text>}
                description={
                  <Space wrap size="small">
                    <Tag>{project.courseName || FIXED_COURSE_NAME}</Tag>
                    <Text type="secondary">{project.goalSummary || "暂无目标摘要"}</Text>
                    <Text type="secondary">revision {project.revision}</Text>
                    <Text type="secondary">更新：{project.updatedAt}</Text>
                  </Space>
                }
              />
            </List.Item>
          )}
        />
      </Modal>

      <Modal
        title="选择文档中心资料"
        open={documentPickerOpen}
        footer={null}
        width={820}
        onCancel={() => setDocumentPickerOpen(false)}
      >
        <Space direction="vertical" className="w-full">
          <Alert
            type="info"
            showIcon
            message="只显示当前账号下可关联的文档"
            description="选择后只建立项目与文档的关联，不复制文件，也不保存本地路径。"
          />
          <Button
            icon={<RefreshCw size={14} />}
            loading={loadingAvailableDocuments}
            onClick={() => void handleLoadAvailableDocuments(false)}
          >
            刷新文档中心资料
          </Button>
          <List
            bordered
            loading={loadingAvailableDocuments}
            dataSource={documentPickerCandidates}
            locale={{ emptyText: "暂无可关联的文档中心资料，或当前文档都已关联。" }}
            renderItem={(note) => (
              <List.Item
                actions={[
                  <Button
                    key="link"
                    type="primary"
                    size="small"
                    loading={linkingDocumentId === note.account_document_id}
                    onClick={() => void handleLinkExistingDocument(note)}
                  >
                    关联
                  </Button>,
                ]}
              >
                <List.Item.Meta
                  avatar={<FileText size={18} />}
                  title={
                    <Space size="small" wrap>
                      <Text>{note.title}</Text>
                      <Tag>{note.document_kind === "uploaded_file" ? "上传文件" : "Markdown"}</Tag>
                    </Space>
                  }
                  description={
                    <Space direction="vertical" size={2}>
                      <Space size="small" wrap>
                        <Text type="secondary">更新：{note.updated_at}</Text>
                        {note.account_file ? (
                          <Text type="secondary">
                            {note.account_file.originalName} · {note.account_file.mimeType || "未知 MIME"}
                          </Text>
                        ) : null}
                      </Space>
                      <Text type="secondary">
                        {note.document_kind === "uploaded_file"
                          ? "关联后可在项目资料中检查解析状态。"
                          : "Markdown 文档可直接作为项目资料线索。"}
                      </Text>
                    </Space>
                  }
                />
              </List.Item>
            )}
          />
        </Space>
      </Modal>
    </div>
  );
}

function TaskGroup({ title, items }: { title: string; items: string[] }) {
  if (!items.length) return null;
  return (
    <div>
      <Text strong>{title}</Text>
      <ul className="mb-0 mt-2 list-disc pl-5">
        {items.map((item) => (
          <li key={item}>
            <Text>{item}</Text>
          </li>
        ))}
      </ul>
    </div>
  );
}

function DiagnosisTagGroup({
  title,
  items,
  color,
  emptyText,
}: {
  title: string;
  items: string[];
  color: string;
  emptyText: string;
}) {
  return (
    <div>
      <Text strong>{title}</Text>
      <div className="mt-2 flex flex-wrap gap-2">
        {items.length ? (
          items.map((item) => (
            <Tag key={item} color={color}>
              {item}
            </Tag>
          ))
        ) : (
          <Text type="secondary">{emptyText}</Text>
        )}
      </div>
    </div>
  );
}

function FallbackResourcesPanel({
  resources,
  disabled,
  onAdd,
}: {
  resources: LearningAssistantFallbackResource[];
  disabled: boolean;
  onAdd: (resource: LearningAssistantFallbackResource) => void;
}) {
  return (
    <Card title="资源推荐">
      <Space direction="vertical" className="w-full">
        <Alert
          type="info"
          showIcon
          message="本地 fallback resources"
          description="推荐来自内置小型静态资源清单，不调用外部平台；加入计划前需要手动确认。"
        />
        <List
          size="small"
          dataSource={resources}
          locale={{ emptyText: "生成计划或完成测试后会按薄弱点推荐本地资源。" }}
          renderItem={(resource) => (
            <List.Item
              actions={[
                <Button
                  key="add"
                  size="small"
                  icon={<CheckCircle2 size={13} />}
                  disabled={disabled}
                  onClick={() => onAdd(resource)}
                >
                  加入计划
                </Button>,
              ]}
            >
              <List.Item.Meta
                avatar={<BookOpen size={18} />}
                title={
                  <Space size="small" wrap>
                    <Text strong>{resource.title}</Text>
                    <Tag>{formatFallbackResourceType(resource.type)}</Tag>
                    <Tag color={difficultyTagColor(resource.difficulty)}>
                      {formatQuizDifficulty(resource.difficulty)}
                    </Tag>
                  </Space>
                }
                description={
                  <Space direction="vertical" size={2}>
                    <Text type="secondary">{resource.summary}</Text>
                    <Space size="small" wrap>
                      <Tag color="blue">{resource.knowledgePoint}</Tag>
                      <Text type="secondary">{resource.duration}</Text>
                      <Text type="secondary">{resource.reason}</Text>
                    </Space>
                  </Space>
                }
              />
            </List.Item>
          )}
        />
      </Space>
    </Card>
  );
}

function DocumentParseStatusLine({ check }: { check?: LearningDocumentParseCheck }) {
  if (!check) {
    return <Text type="secondary">解析状态：未检查</Text>;
  }
  return (
    <Space size="small" wrap>
      <Tag color={parseStatusTagColor(check.status)}>{formatParseStatus(check.status)}</Tag>
      <Text type="secondary">{check.detail}</Text>
      {check.checkedAt ? <Text type="secondary">{check.checkedAt}</Text> : null}
    </Space>
  );
}

function QaRecordList({ records }: { records: LearningAssistantQaRecord[] }) {
  if (!records.length) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description="当前项目暂无知识库问答记录。"
      />
    );
  }
  return (
    <List
      size="small"
      dataSource={records}
      renderItem={(record) => (
        <List.Item>
          <Space direction="vertical" className="w-full" size="small">
            <Space wrap size="small">
              <Text strong>{record.question}</Text>
              <Tag color={record.generationType === LEARNING_ASSISTANT_QA_SOURCE ? "green" : "orange"}>
                {record.generationType === LEARNING_ASSISTANT_QA_SOURCE ? "本地知识库" : "未命中"}
              </Tag>
              <Text type="secondary">置信度 {Math.round(record.confidence * 100)}%</Text>
              <Text type="secondary">{record.askedAt}</Text>
            </Space>
            <Paragraph className="!mb-0 whitespace-pre-line">{record.answer}</Paragraph>
            <QaSourceList sources={record.sources} />
          </Space>
        </List.Item>
      )}
    />
  );
}

function QaSourceList({ sources }: { sources: LearningAssistantQaSource[] }) {
  if (!sources.length) {
    return <Text type="secondary">暂无可追溯来源。</Text>;
  }
  return (
    <Space wrap size="small">
      {sources.map((source) => (
        <Tag key={source.sourceKey} color={source.sourceKind === "knowledgeBase" ? "blue" : "purple"}>
          {source.sourceKind === "knowledgeBase"
            ? `${source.title} · ${source.sourceFile ?? "Excel"}`
            : `${source.title}${source.status === "deleted" ? " · 已删除" : ""}`}
        </Tag>
      ))}
    </Space>
  );
}

function LearningProgressOverviewPanel({
  overview,
}: {
  overview: LearningAssistantProgressOverview;
}) {
  const progressStatus =
    overview.stageCount > 0 && overview.completedStageCount === overview.stageCount
      ? "success"
      : overview.needsReviewStageCount
        ? "exception"
        : "active";

  return (
    <Space direction="vertical" className="w-full">
      <Progress
        percent={overview.progressPercent}
        status={progressStatus}
        format={(percent) => `${percent ?? 0}%`}
      />
      <Space wrap>
        <Tag color="blue">阶段 {overview.completedStageCount}/{overview.stageCount}</Tag>
        <Tag color="purple">测试 {overview.quizRecordCount}</Tag>
        <Tag color="green">已掌握 {overview.mastery.mastered}</Tag>
        <Tag color="orange">薄弱 {overview.mastery.weak}</Tag>
        <Tag>资料 {overview.linkedDocumentCount}</Tag>
      </Space>
      {overview.stageStatuses.length ? (
        <List
          size="small"
          dataSource={overview.stageStatuses}
          renderItem={(stage) => (
            <List.Item>
              <Space direction="vertical" className="w-full" size={2}>
                <Space wrap size="small">
                  <Text strong>
                    {stage.stageIndex + 1}. {stage.stageName}
                  </Text>
                  <Tag color={stageProgressTagColor(stage.status)}>
                    {formatStageProgressStatus(stage.status)}
                  </Tag>
                  {stage.latestPercentage !== null ? (
                    <Text type="secondary">最近测试 {stage.latestPercentage}%</Text>
                  ) : null}
                </Space>
                {stage.weakKnowledgePoints.length ? (
                  <Text type="secondary">
                    薄弱点：{stage.weakKnowledgePoints.join("、")}
                  </Text>
                ) : null}
              </Space>
            </List.Item>
          )}
        />
      ) : (
        <Alert type="info" showIcon message="生成学习计划后会显示阶段进度。" />
      )}
      <RecentActivityList activities={overview.recentActivities} />
    </Space>
  );
}

function RecentActivityList({ activities }: { activities: LearningAssistantProgressActivity[] }) {
  if (!activities.length) {
    return <Text type="secondary">暂无最近学习活动。</Text>;
  }
  return (
    <Timeline
      className="!mt-2"
      items={activities.map((activity) => ({
        color: activityTimelineColor(activity.activityType),
        children: (
          <Space direction="vertical" size={0}>
            <Space wrap size="small">
              <Tag color={activityTagColor(activity.activityType)}>
                {formatActivityType(activity.activityType)}
              </Tag>
              <Text>{activity.message}</Text>
            </Space>
            <Text type="secondary">{activity.occurredAt}</Text>
          </Space>
        ),
      }))}
    />
  );
}

function StageQuizPanel({
  session,
  onRetake,
  onAnswerChange,
  onSubmit,
}: {
  session?: StageQuizSession;
  onRetake: () => void;
  onAnswerChange: (questionKey: string, answer: string) => void;
  onSubmit: () => void;
}) {
  if (!session) return null;
  if (!session.questions.length) {
    return <Alert type="info" showIcon message={session.message || "当前阶段暂无可用测试题。"} />;
  }

  return (
    <Card
      size="small"
      title="阶段测试"
      extra={
        <Button size="small" icon={<RotateCcw size={13} />} onClick={onRetake}>
          重测
        </Button>
      }
    >
      <Space direction="vertical" className="w-full">
        {session.message ? <Alert type="info" showIcon message={session.message} /> : null}
        <Space wrap size="small">
          <Tag color="blue">
            已答 {Object.values(session.answers).filter((answer) => answer.trim()).length}/{session.questions.length}
          </Tag>
          {session.scoreResult ? (
            <Tag color={session.scoreResult.canAdvance ? "green" : "orange"}>
              {session.scoreResult.canAdvance ? "可进入下一阶段" : "建议复测"}
            </Tag>
          ) : (
            <Tag>待提交</Tag>
          )}
        </Space>
        <List
          size="small"
          dataSource={session.questions}
          renderItem={(question, questionIndex) => (
            <List.Item>
              <Space direction="vertical" className="w-full">
                <Space wrap size="small">
                  <Tag color="processing">第 {questionIndex + 1} 题</Tag>
                  <Tag>{formatQuizQuestionType(question.type)}</Tag>
                  <Tag color={difficultyTagColor(question.difficulty)}>
                    {formatQuizDifficulty(question.difficulty)}
                  </Tag>
                  <Tag color="blue">{question.score} 分</Tag>
                  <Tag color="green">{question.knowledgePoint}</Tag>
                </Space>
                <Text strong>{question.question}</Text>
                <Space wrap size="small">
                  <Text type="secondary">来源：{question.sourceTitle || "本地 fallback 题库"}</Text>
                  {question.sourceFile ? <Text type="secondary">文件：{question.sourceFile}</Text> : null}
                </Space>
                {question.type === "choice" || question.type === "judgment" ? (
                  <Radio.Group
                    value={session.answers[question.questionKey]}
                    onChange={(event) => onAnswerChange(question.questionKey, event.target.value)}
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
                    value={session.answers[question.questionKey] ?? ""}
                    autoSize={{ minRows: 2, maxRows: 5 }}
                    placeholder="请输入你的答案"
                    onChange={(event) => onAnswerChange(question.questionKey, event.target.value)}
                  />
                )}
              </Space>
            </List.Item>
          )}
        />
        <Button type="primary" loading={session.scoring} onClick={onSubmit}>
          提交测试并保存
        </Button>
        {session.scoreResult ? <QuizScoreResultView result={session.scoreResult} /> : null}
      </Space>
    </Card>
  );
}

function QuizScoreResultView({ result }: { result: LearningAssistantQuizScoreResult }) {
  return (
    <Card size="small" title="评分结果">
      <Space direction="vertical" className="w-full">
        <Space wrap>
          <Tag color="blue">
            总分：{result.totalScore} / {result.maxScore}
          </Tag>
          <Tag color="cyan">百分制：{result.percentage} 分</Tag>
          <Tag color={result.canAdvance ? "green" : "orange"}>{result.level}</Tag>
          <Tag color={result.detailResults.some((item) => !item.correct) ? "red" : "green"}>
            错题 {result.detailResults.filter((item) => !item.correct).length}
          </Tag>
        </Space>
        <Alert type={result.canAdvance ? "success" : "warning"} showIcon message={result.feedback} />
        <Descriptions column={1} size="small" bordered>
          <Descriptions.Item label="薄弱知识点">
            {result.weakKnowledgePoints.length ? result.weakKnowledgePoints.join("、") : "暂无明显薄弱点"}
          </Descriptions.Item>
          <Descriptions.Item label="缺失关键词">
            {result.missingKeywords.length ? result.missingKeywords.join("、") : "暂无"}
          </Descriptions.Item>
          <Descriptions.Item label="复习建议">{result.suggestions.join("；")}</Descriptions.Item>
        </Descriptions>
      </Space>
    </Card>
  );
}

function MasteryRecordSummary({ records }: { records: LearningAssistantMasteryRecord[] }) {
  if (!records.length) {
    return <Alert type="info" showIcon message="提交阶段测试后，将按知识点生成掌握度记录。" />;
  }
  return (
    <Space direction="vertical" className="w-full">
      <Space wrap>
        <Tag color="green">
          已掌握 {records.filter((record) => record.masteryLevel === "mastered").length}
        </Tag>
        <Tag color="blue">
          基本掌握 {records.filter((record) => record.masteryLevel === "basic").length}
        </Tag>
        <Tag color="orange">
          薄弱 {records.filter((record) => record.masteryLevel === "weak").length}
        </Tag>
      </Space>
      <List
        size="small"
        dataSource={records.slice(0, 8)}
        renderItem={(record) => (
          <List.Item>
            <Space direction="vertical" className="w-full" size={0}>
              <Space wrap size="small">
                <Text strong>{record.knowledgePoint}</Text>
                <Tag color={masteryTagColor(record.masteryLevel)}>
                  {formatMasteryLevel(record.masteryLevel)}
                </Tag>
                <Text type="secondary">
                  最近 {record.latestPercentage} 分，最佳 {record.bestPercentage} 分，测试 {record.attempts} 次
                </Text>
              </Space>
              <Text type="secondary">{record.suggestions.join("；")}</Text>
            </Space>
          </List.Item>
        )}
      />
    </Space>
  );
}

function ReplanSuggestionPanel({
  records,
  onApply,
  applying,
}: {
  records: LearningAssistantQuizRecord[];
  onApply: (record: LearningAssistantQuizRecord) => void;
  applying: boolean;
}) {
  if (!records.length) return null;
  return (
    <Alert
      type="warning"
      showIcon
      message="有待确认的重新规划建议"
      description={
        <Space direction="vertical" className="w-full">
          {records.map((record) => (
            <Space key={record.recordKey} wrap>
              <Text strong>{record.stageName}</Text>
              <Tag color="orange">{record.percentage} 分</Tag>
              <Text type="secondary">
                薄弱点：{record.weakKnowledgePoints.length ? record.weakKnowledgePoints.join("、") : "核心知识点"}
              </Text>
              <Button
                size="small"
                type="primary"
                loading={applying}
                icon={<ClipboardList size={13} />}
                onClick={() => onApply(record)}
              >
                确认并写入计划
              </Button>
            </Space>
          ))}
        </Space>
      }
    />
  );
}

function QuizRecordList({
  records,
  appliedRecordKeys,
  onRetake,
}: {
  records: LearningAssistantQuizRecord[];
  appliedRecordKeys: Set<string>;
  onRetake: (record: LearningAssistantQuizRecord) => void;
}) {
  if (!records.length) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description="当前项目暂无阶段测试记录。"
      />
    );
  }
  return (
    <List
      size="small"
      dataSource={records}
      renderItem={(record) => (
        <List.Item>
          <Space direction="vertical" className="w-full" size="small">
            <Space wrap size="small">
              <Text strong>{record.stageName}</Text>
              <Tag color={record.canAdvance ? "green" : "orange"}>{record.level}</Tag>
              <Tag color="blue">
                {record.totalScore}/{record.maxScore} · {record.percentage} 分
              </Tag>
              <Tag color={record.canAdvance ? "green" : "orange"}>
                {record.canAdvance
                  ? "已通过"
                  : appliedRecordKeys.has(record.recordKey)
                    ? "已重新规划"
                    : "待复测"}
              </Tag>
              <Text type="secondary">{record.testedAt}</Text>
              <Button size="small" icon={<RotateCcw size={13} />} onClick={() => onRetake(record)}>
                重测本阶段
              </Button>
            </Space>
            <Paragraph className="!mb-0">{record.feedback}</Paragraph>
            <TaskGroup title="复习建议" items={record.suggestions} />
            <QuizRecordItems record={record} />
          </Space>
        </List.Item>
      )}
    />
  );
}

function QuizRecordItems({ record }: { record: LearningAssistantQuizRecord }) {
  return (
    <List
      size="small"
      dataSource={record.items}
      locale={{ emptyText: "暂无题目明细" }}
      renderItem={(item, index) => (
        <List.Item>
          <Space direction="vertical" className="w-full" size={0}>
            <Space wrap size="small">
              <Tag>第 {index + 1} 题</Tag>
              <Tag>{formatQuizQuestionType(item.questionType)}</Tag>
              <Tag color={item.correct ? "green" : "red"}>
                {item.score}/{item.maxScore} 分
              </Tag>
              <Tag color={item.correct ? "green" : "red"}>
                {item.correct ? "正确" : "错题"}
              </Tag>
              <Tag color={difficultyTagColor(item.difficulty)}>
                {formatQuizDifficulty(item.difficulty)}
              </Tag>
              <Tag color="purple">{item.knowledgePoint}</Tag>
            </Space>
            <Text>{item.question}</Text>
            <Text type="secondary">你的答案：{item.userAnswer || "未作答"}</Text>
            <Text type="secondary">标准答案：{item.standardAnswer}</Text>
            {!item.correct && item.missingKeywords.length ? (
              <Text type="secondary">缺失关键词：{item.missingKeywords.join("、")}</Text>
            ) : null}
            <Text type="secondary">
              来源：{item.sourceTitle || "本地 fallback 题库"}
              {item.sourceFile ? ` · ${item.sourceFile}` : ""}
            </Text>
            <Text type="secondary">解析：{item.explanation}</Text>
          </Space>
        </List.Item>
      )}
    />
  );
}

function formatQuizDifficulty(difficulty: LearningAssistantQuizQuestion["difficulty"]) {
  if (difficulty === "easy") return "基础";
  if (difficulty === "hard") return "进阶";
  return "中等";
}

function difficultyTagColor(difficulty: LearningAssistantQuizQuestion["difficulty"]) {
  if (difficulty === "easy") return "green";
  if (difficulty === "hard") return "volcano";
  return "gold";
}

function formatFallbackResourceType(type: LearningAssistantFallbackResource["type"]) {
  if (type === "courseware") return "课件";
  if (type === "case") return "案例";
  if (type === "checklist") return "清单";
  if (type === "exercise") return "练习";
  return "参考";
}

function formatParseStatus(status: LearningDocumentParseStatus) {
  if (status === "ready") return "可解析";
  if (status === "unsupported") return "未读到正文";
  if (status === "unavailable") return "不可用";
  return "检查中";
}

function parseStatusTagColor(status: LearningDocumentParseStatus) {
  if (status === "ready") return "green";
  if (status === "unsupported") return "orange";
  if (status === "unavailable") return "red";
  return "blue";
}

function formatQuizQuestionType(type: LearningAssistantQuizQuestion["type"]) {
  if (type === "choice") return "选择题";
  if (type === "judgment") return "判断题";
  return "简答题";
}

function formatMasteryLevel(level: LearningAssistantMasteryRecord["masteryLevel"]) {
  if (level === "mastered") return "已掌握";
  if (level === "basic") return "基本掌握";
  return "薄弱";
}

function masteryTagColor(level: LearningAssistantMasteryRecord["masteryLevel"]) {
  if (level === "mastered") return "green";
  if (level === "basic") return "blue";
  return "orange";
}

function formatStageProgressStatus(status: LearningAssistantStageProgress["status"]) {
  if (status === "completed") return "已完成";
  if (status === "needsReview") return "需复习";
  if (status === "inProgress") return "进行中";
  return "未开始";
}

function stageProgressTagColor(status: LearningAssistantStageProgress["status"]) {
  if (status === "completed") return "green";
  if (status === "needsReview") return "orange";
  if (status === "inProgress") return "blue";
  return "default";
}

function formatActivityType(type: LearningAssistantProgressActivity["activityType"]) {
  if (type === "qa") return "问答";
  if (type === "quiz") return "测试";
  if (type === "replan") return "调整";
  if (type === "document") return "资料";
  return "项目";
}

function activityTagColor(type: LearningAssistantProgressActivity["activityType"]) {
  if (type === "qa") return "cyan";
  if (type === "quiz") return "purple";
  if (type === "replan") return "orange";
  if (type === "document") return "blue";
  return "default";
}

function activityTimelineColor(type: LearningAssistantProgressActivity["activityType"]) {
  if (type === "qa") return "cyan";
  if (type === "quiz") return "purple";
  if (type === "replan") return "orange";
  if (type === "document") return "blue";
  return "gray";
}

function buildPlanInput(values: LearningGoalFormValues) {
  return {
    learningAssistantRoot: null,
    learningGoal: values.learningGoal,
    courseName: values.courseName,
    learningCycle: values.learningCycle,
    dailyTime: formatStudyHours(values.dailyStudyHours),
    dailyStudyHours: values.dailyStudyHours,
    currentLevel: values.currentLevel,
    finalGoal: values.finalGoal,
  };
}

function buildKnowledgeQuestionInput(
  values: Partial<LearningGoalFormValues>,
  currentPlan: LearningAssistantPlanResult | null,
  question: string,
) {
  const stages = currentPlan?.stages ?? [];
  return {
    course: values.courseName ?? FIXED_COURSE_NAME,
    query: question,
    stageName: "",
    stageIndex: 0,
    stageGoal: values.learningGoal ?? "",
    learningTasks: flattenStageTasks(stages, "learningTasks"),
    resourceTasks: flattenStageTasks(stages, "resourceTasks"),
    practiceTasks: flattenStageTasks(stages, "practiceTasks"),
    checkTasks: flattenStageTasks(stages, "checkTasks"),
    knowledgePoints: collectPlanKnowledgePoints(currentPlan),
    topK: 5,
  };
}

function collectPlanKnowledgePoints(currentPlan: LearningAssistantPlanResult | null): string[] {
  const points = new Set<string>();
  for (const stage of currentPlan?.stages ?? []) {
    for (const point of stage.knowledgePoints ?? []) {
      if (point.trim()) points.add(point.trim());
    }
    for (const entry of stage.learningEntries ?? []) {
      if (entry.title.trim()) points.add(entry.title.trim());
      if (entry.section.trim()) points.add(entry.section.trim());
    }
  }
  return [...points].slice(0, 30);
}

function flattenStageTasks(
  stages: LearningAssistantStage[],
  key: "learningTasks" | "resourceTasks" | "practiceTasks" | "checkTasks",
): string[] {
  return stages
    .flatMap((stage) => stage[key])
    .filter((task) => task.trim())
    .slice(0, 30);
}

function goalSnapshotFromValues(values: LearningGoalFormValues) {
  return {
    courseName: values.courseName,
    learningGoal: values.learningGoal,
    learningCycle: values.learningCycle,
    dailyStudyHours: values.dailyStudyHours,
    currentLevel: values.currentLevel,
    finalGoal: values.finalGoal,
  };
}

function buildDraftUnderstanding(values: LearningGoalFormValues): LearningAssistantUnderstanding {
  const dailyTime = formatStudyHours(values.dailyStudyHours);
  return {
    summary: `围绕「${values.courseName}」在${values.learningCycle}内完成「${values.learningGoal}」，每天投入${dailyTime}。`,
    currentGap: `当前基础为「${values.currentLevel}」，需要生成计划后结合本地知识点进一步确认已掌握、待学习和薄弱知识点。`,
    strategy: "先完成初始诊断，再生成 fallback 学习计划，随后通过问答、测试和复盘持续更新掌握度。",
    closedLoop: "目标设定 → 初始诊断 → 计划生成 → 学习执行 → 测试反馈 → 计划调整。",
    source: FALLBACK_SOURCE,
  };
}

function buildLearningGoal(values: LearningGoalFormValues): JsonObject {
  return {
    courseName: values.courseName,
    learningGoal: values.learningGoal,
    learningCycle: values.learningCycle,
    dailyStudyHours: values.dailyStudyHours,
    dailyTime: formatStudyHours(values.dailyStudyHours),
    currentLevel: values.currentLevel,
    finalGoal: values.finalGoal,
    source: FALLBACK_SOURCE,
  };
}

function buildGoalSummary(values: LearningGoalFormValues): string {
  return `${values.learningGoal} · ${values.learningCycle} · ${formatStudyHours(values.dailyStudyHours)} · ${values.finalGoal}`;
}

function buildProgress(
  status: "draft" | "planned",
  currentPlan: LearningAssistantPlanResult | null,
  linkedDocumentCount: number,
  diagnosis: LearningAssistantDiagnosis | null,
  previousProgress?: JsonObject | null,
): JsonObject {
  const previous = isRecord(previousProgress) ? previousProgress : {};
  return {
    ...previous,
    status,
    source: FALLBACK_SOURCE,
    stageCount: currentPlan?.stages.length ?? 0,
    linkedDocumentCount,
    diagnosisStatus: diagnosis ? "available" : "pending",
    masteredKnowledgeCount: diagnosis?.masteredKnowledgePoints.length ?? 0,
    pendingKnowledgeCount: diagnosis?.pendingKnowledgePoints.length ?? 0,
    weakKnowledgeCount: diagnosis?.weakKnowledgePoints.length ?? 0,
    updatedAt: new Date().toISOString(),
  };
}

function formValuesFromProject(project: LearningProjectDetail): LearningGoalFormValues {
  return {
    name: project.name,
    courseName: readString(project.learningGoal, "courseName", project.courseName ?? FIXED_COURSE_NAME),
    learningGoal: readString(project.learningGoal, "learningGoal", DEFAULT_VALUES.learningGoal),
    learningCycle: readString(project.learningGoal, "learningCycle", DEFAULT_VALUES.learningCycle),
    dailyStudyHours: readNumber(project.learningGoal, "dailyStudyHours", DEFAULT_VALUES.dailyStudyHours),
    currentLevel: readString(project.learningGoal, "currentLevel", DEFAULT_VALUES.currentLevel),
    finalGoal: readString(project.learningGoal, "finalGoal", DEFAULT_VALUES.finalGoal),
  };
}

function inferProjectName(values: LearningGoalFormValues): string {
  return `${values.courseName || FIXED_COURSE_NAME} · ${values.learningGoal || "学习项目"}`;
}

function summaryFromDetail(project: LearningProjectDetail): LearningProjectSummary {
  return {
    id: project.id,
    name: project.name,
    learningType: project.learningType,
    courseName: project.courseName,
    goalSummary: project.goalSummary,
    revision: project.revision,
    lastOpenedAt: project.lastOpenedAt,
    createdAt: project.createdAt,
    updatedAt: project.updatedAt,
  };
}

function upsertProjectSummary(
  projects: LearningProjectSummary[],
  project: LearningProjectSummary,
): LearningProjectSummary[] {
  return [project, ...projects.filter((item) => item.id !== project.id)].sort((a, b) =>
    b.updatedAt.localeCompare(a.updatedAt),
  );
}

function upsertDocument(
  documents: LearningProjectDocument[],
  document: LearningProjectDocument,
): LearningProjectDocument[] {
  return [document, ...documents.filter((item) => item.documentId !== document.documentId)];
}

async function listAccountBackedNotes(): Promise<AccountBackedNote[]> {
  const result = await noteApi.list({ page: 1, page_size: 100, sort_by: "default" });
  return result.items.filter(isAccountBackedNote);
}

async function findAccountNoteByDocumentId(
  documentId: string,
  cachedNotes: AccountBackedNote[],
): Promise<AccountBackedNote | null> {
  const cached = cachedNotes.find((note) => note.account_document_id === documentId);
  if (cached) return cached;
  const notes = await listAccountBackedNotes();
  return notes.find((note) => note.account_document_id === documentId) ?? null;
}

function isAccountBackedNote(note: unknown): note is AccountBackedNote {
  return (
    isRecord(note) &&
    typeof note.account_document_id === "string" &&
    typeof note.document_kind === "string" &&
    typeof note.title === "string" &&
    typeof note.updated_at === "string"
  );
}

function addFallbackResourceToPlan(
  currentPlan: LearningAssistantPlanResult,
  resource: LearningAssistantFallbackResource,
): LearningAssistantPlanResult {
  if (!currentPlan.stages.length) return currentPlan;
  const stageIndex = findStageIndexForResource(currentPlan, resource);
  const resourceTask = `推荐资料：${resource.title}（${resource.knowledgePoint}，${resource.duration}）`;
  const targetStage = currentPlan.stages[stageIndex];
  if (targetStage.resourceTasks.some((task) => task.includes(resource.title))) return currentPlan;
  const stages = currentPlan.stages.map((stage, index) =>
    index === stageIndex
      ? {
          ...stage,
          resourceTasks: mergeUniqueTasks(stage.resourceTasks, [resourceTask]),
          checkTasks: mergeUniqueTasks(stage.checkTasks, [`阅读「${resource.title}」后，用 3 句话复述关键结论。`]),
        }
      : stage,
  );
  return {
    ...currentPlan,
    stages,
    message: `已将推荐资源「${resource.title}」加入「${targetStage.name}」。`,
  };
}

function findStageIndexForResource(
  currentPlan: LearningAssistantPlanResult,
  resource: LearningAssistantFallbackResource,
): number {
  const normalizedPoint = resource.knowledgePoint.trim();
  const matchIndex = currentPlan.stages.findIndex((stage) =>
    [
      stage.goal,
      ...stage.learningTasks,
      ...stage.resourceTasks,
      ...stage.practiceTasks,
      ...(stage.knowledgePoints ?? []),
      ...(stage.learningEntries ?? []).map((entry) => `${entry.title} ${entry.section}`),
    ].some((value) => value.includes(normalizedPoint)),
  );
  return matchIndex >= 0 ? matchIndex : 0;
}

function mergeUniqueTasks(existing: string[], additions: string[]): string[] {
  const merged = [...existing];
  for (const addition of additions) {
    const normalized = addition.trim();
    if (normalized && !merged.some((task) => task.trim() === normalized)) {
      merged.push(normalized);
    }
  }
  return merged;
}

function hasPlanAdjustment(adjustments: unknown, adjustmentKey: string): boolean {
  if (!Array.isArray(adjustments)) return false;
  return adjustments.some(
    (adjustment) =>
      isRecord(adjustment) &&
      typeof adjustment.adjustmentKey === "string" &&
      adjustment.adjustmentKey === adjustmentKey,
  );
}

function isLearningPlan(value: unknown): value is LearningAssistantPlanResult {
  if (!isRecord(value)) return false;
  return (
    typeof value.success === "boolean" &&
    isRecord(value.understanding) &&
    Array.isArray(value.stages)
  );
}

function toJsonObject(value: unknown): JsonObject {
  const cloned = JSON.parse(JSON.stringify(value ?? {})) as unknown;
  return isRecord(cloned) ? cloned : {};
}

function readString(source: JsonObject, key: string, fallback: string): string {
  const value = source[key];
  return typeof value === "string" && value.trim() ? value : fallback;
}

function readNumber(source: JsonObject, key: string, fallback: number): number {
  const value = source[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function formatStudyHours(hours: number): string {
  const safeHours = Number.isFinite(hours) && hours > 0 ? hours : 1;
  const whole = Math.floor(safeHours);
  const minutes = Math.round((safeHours - whole) * 60);
  if (!whole) return `每天${minutes}分钟`;
  return minutes ? `每天${whole}小时${minutes}分钟` : `每天${whole}小时`;
}

function formatLearningError(error: unknown, fallback: string): string {
  if (isRecord(error)) {
    const code = typeof error.code === "string" ? error.code : "";
    if (code === "signedOut") return "请先登录。";
    if (code === "learningProjectConflict") return "项目已在其他设备更新，请刷新后再保存。";
    if (code === "learningProjectDocumentExists") return "该资料已经关联到当前项目。";
    if (code === "learningProjectNotFound" || code === "learningProjectDocumentNotFound") {
      return "项目或资料不存在，或不属于当前账号。";
    }
    if (code === "validation") return "请求参数不符合要求，请检查输入。";
    if (code === "unavailable") return "账号服务暂不可用，请稍后刷新确认结果。";
    if (typeof error.message === "string" && error.message.trim()) {
      return error.message;
    }
  }
  if (typeof error === "string" && error.trim()) return error;
  return fallback;
}
