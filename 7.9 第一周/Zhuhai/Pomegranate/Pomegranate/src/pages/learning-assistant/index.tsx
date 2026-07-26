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
  InputNumber,
  List,
  Modal,
  Popconfirm,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
  message,
} from "antd";
import {
  BookOpen,
  Copy,
  FileText,
  FolderOpen,
  GraduationCap,
  Link2,
  PenLine,
  RefreshCw,
  Save,
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
  const [qaQuestion, setQaQuestion] = useState("");
  const [askingQuestion, setAskingQuestion] = useState(false);

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

  useEffect(() => {
    if (!currentUser) {
      setProjects([]);
      setCurrentProject(null);
      setDocuments([]);
      setPlan(null);
      setCheckResult(null);
      setQaQuestion("");
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

  function openProjectDetail(project: LearningProjectDetail) {
    setCurrentProject(project);
    form.setFieldsValue(formValuesFromProject(project));
    setPlan(isLearningPlan(project.currentPlan) ? project.currentPlan : null);
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
                            <Space size="small" wrap>
                              <Tag>{document.documentType}</Tag>
                              <Tag>{document.role}</Tag>
                              <Text type="secondary">排序 {document.sortOrder}</Text>
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
                  <Form.Item name="dailyStudyHours" label="每日投入小时" rules={[{ required: true }]}>
                    <InputNumber className="w-full" min={0.5} max={12} step={0.5} />
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
                            <TaskGroup title="练习任务" items={stage.practiceTasks} />
                            <TaskGroup title="检查标准" items={stage.completionCriteria} />
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
