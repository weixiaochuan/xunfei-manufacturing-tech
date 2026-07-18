import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Empty,
  Form,
  Input,
  InputNumber,
  List,
  Modal,
  Radio,
  Segmented,
  Select,
  Space,
  Spin,
  Steps,
  Switch,
  Tag,
  Tabs,
  Typography,
  message,
} from "antd";
import {
  CalendarDays,
  FileDown,
  ExternalLink,
  FileText,
  FolderOpen,
  Play,
  RotateCcw,
  Search,
  SearchCheck,
  Trash2,
  Wand2,
} from "lucide-react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { aiModelApi, aiWriteApi, configApi, folderApi, noteApi, pptMasterApi, systemApi } from "@/lib/api";
import {
  hasSubstantivePptMaterial,
  mergePptMaterialSources,
  normalizePptMaterialText,
} from "@/lib/pptMaterial";
import {
  calculatePptContextBudget,
  resolvePptReservedOutputTokens,
} from "@/lib/pptContextBudget";
import {
  buildPptChunkUnderstandingPrompt,
  buildPptUnderstandingMergePrompt,
  parsePptChunkUnderstandingResponse,
  parsePptUnderstandingMergeResponse,
  type PptChunkUnderstandingContext,
} from "@/lib/pptChunkUnderstandingPrompt";
import { executePptChunkUnderstandingWorkflow } from "@/lib/pptChunkUnderstandingWorkflow";
import {
  formatPptAnalysisProgress,
  formatPptFailedParts,
  formatPptFeeIntroduction,
  PPT_UNDERSTANDING_UI_COPY,
  PPT_UNDERSTANDING_FIELD_DESCRIPTIONS,
} from "@/lib/pptUnderstandingUi";
import { preparePptUnderstandingDraftForDisplay } from "@/lib/pptUnderstandingFormatting";
import {
  planPptMaterialChunks,
  resolvePptMaterialRequestPlan,
} from "@/lib/pptMaterialChunking";
import {
  buildAiUnderstandingPrompt,
  buildAiUnderstandingPromptParts,
  type PptUnderstandingPromptInput,
} from "@/lib/pptUnderstandingPrompt";
import { buildPptUnderstandingMarkdown } from "@/lib/pptUnderstandingExport";
import { useTabsStore } from "@/store/tabs";
import {
  getEffectivePptRawMaterial,
  usePptGenerationDraftStore,
  type PptMaterialInputMode,
  type PptSmartDraftFields,
} from "@/store/pptGenerationDraft";
import {
  dailySnapshotHasUnpersistedContent,
  getDailyDraftSnapshot,
} from "@/services/dailyDraftBridge";
import type {
  AiModel,
  Folder,
  Note,
  PptMasterCheckResult,
  PptMasterExportResult,
  PptMasterGenerateInput,
  PptMaterialChunkPlan,
  PptMaterialSourceRef,
  PptMaterialSourceType,
  PptUnderstandingDraft,
  ResolvedPptMaterialSource,
} from "@/types";

const { Text, Title } = Typography;

const CONFIG_KEYS = {
  pptMasterRoot: "ppt_master.root",
  pythonPath: "ppt_master.python_path",
  defaultProjectPath: "ppt_master.default_project_path",
  aiModelId: "ppt_generation.ai_model_id",
  outputDir: "ppt_generation.output_dir",
} as const;

interface AdvancedFormValues {
  pptMasterRoot: string;
  pythonPath: string;
  projectPath: string;
}

type SmartFormValues = PptSmartDraftFields;
type FinalSmartValues = PptUnderstandingPromptInput;

type MaterialPickerFilter = "all" | PptMaterialSourceType;

interface MaterialCandidate extends PptMaterialSourceRef {
  note: Note;
  folderPath?: string;
  selectable: boolean;
  disabledReason?: string;
}

const audienceOptions = ["老师/评委", "企业客户", "课堂展示", "项目组内部", "自定义"];
const pageCountOptions = ["4 页", "6 页", "8 页", "10 页", "自定义"];
const styleOptions = ["简约商务", "科技蓝", "学术汇报", "竞赛路演", "图文并茂", "自定义"];
const stylePresets: Record<
  string,
  {
    mode: string;
    visualStyle: string;
    layoutBias: string[];
    chartBias: string[];
    description: string;
  }
> = {
  简约商务: {
    mode: "pyramid",
    visualStyle: "swiss-minimal",
    layoutBias: [],
    chartBias: ["kpi_cards", "comparison_columns", "process_flow"],
    description: "结论先行、结构清晰，适合商业汇报和方案说明。",
  },
  科技蓝: {
    mode: "showcase",
    visualStyle: "dark-tech",
    layoutBias: ["ai_ops"],
    chartBias: ["pipeline_with_stages", "process_flow", "layered_architecture", "kpi_cards"],
    description: "蓝白配色、结构清晰，适合需要现代感和条理化表达的内容。",
  },
  学术汇报: {
    mode: "instructional",
    visualStyle: "data-journalism",
    layoutBias: ["academic_defense"],
    chartBias: ["line_chart", "bar_chart", "basic_table", "timeline"],
    description: "适用于论文汇报、科研成果和实验过程说明。",
  },
  竞赛路演: {
    mode: "narrative",
    visualStyle: "glassmorphism",
    layoutBias: ["ai_ops"],
    chartBias: ["kpi_cards", "process_flow", "timeline", "comparison_columns"],
    description: "适用于创新创业比赛和项目答辩，突出价值、创新点和落地路径。",
  },
  图文并茂: {
    mode: "showcase",
    visualStyle: "photo-editorial",
    layoutBias: [],
    chartBias: ["vertical_list", "journey_map", "kpi_cards"],
    description: "适合图片、案例、场景化内容较多的展示。",
  },
};
const EXAMPLE_PROJECT_MARKER = "examples/ppt169_lin_huiyin_architect_revised";

async function getConfigOrEmpty(key: string): Promise<string> {
  try {
    return await configApi.get(key);
  } catch {
    return "";
  }
}

const understandingFieldDefinitions: Array<{
  key: keyof PptUnderstandingDraft;
  title: string;
  description: string;
  minRows: number;
  maxRows: number;
  required?: boolean;
}> = [
  {
    key: "understandingSummary",
    title: "AI 理解摘要",
    description: "AI 对这份 PPT 目标和任务的总体理解。",
    minRows: 3,
    maxRows: 8,
    required: true,
  },
  {
    key: "keyPriorities",
    title: "重点取舍",
    description: "应该重点讲什么、弱化什么、不要讲什么。",
    minRows: 5,
    maxRows: 12,
  },
  {
    key: "narrativeMainline",
    title: "叙事主线",
    description: "整份 PPT 从开头到结尾的逻辑推进方式。",
    minRows: 3,
    maxRows: 8,
    required: true,
  },
  {
    key: "suggestedPageStructure",
    title: "建议页面结构",
    description: PPT_UNDERSTANDING_FIELD_DESCRIPTIONS.suggestedPageStructure,
    minRows: 7,
    maxRows: 18,
    required: true,
  },
  {
    key: "visualExpressionAdvice",
    title: "视觉与表达建议",
    description: PPT_UNDERSTANDING_FIELD_DESCRIPTIONS.visualExpressionAdvice,
    minRows: 4,
    maxRows: 10,
  },
  {
    key: "openQuestions",
    title: "仍需确认的问题",
    description: PPT_UNDERSTANDING_FIELD_DESCRIPTIONS.openQuestions,
    minRows: 3,
    maxRows: 8,
  },
];

function extractSection(raw: string, title: string): string {
  const escaped = title.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const pattern = new RegExp(`【${escaped}】\\s*([\\s\\S]*?)(?=\\n?【[^】]+】|$)`);
  return pattern.exec(raw)?.[1]?.trim() ?? "";
}

function cleanAiMarkdown(value: string): string {
  return value
    .split(/\r?\n/)
    .map((line) => line.trimEnd())
    .filter((line) => line.trim() !== "---")
    .map((line) =>
      line
        .replace(/^\s*\*\*(.*?)\*\*\s*[:：]?$/g, "$1")
        .replace(/\*\*(.*?)\*\*/g, "$1")
        .replace(/^\s{0,3}#{1,6}\s+/g, "")
        .trimEnd(),
    )
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function normalizePathForCompare(value?: string): string {
  return (value ?? "").replace(/\\/g, "/").toLowerCase();
}

function isLinHuiyinExampleProject(value?: string): boolean {
  return normalizePathForCompare(value).includes(EXAMPLE_PROJECT_MARKER);
}

function getDisplayFolderLabel(path?: string | null): string {
  if (!path) {
    return "";
  }
  const normalized = path.replace(/\\/g, "/");
  const index = normalized.lastIndexOf("/");
  return index > 0 ? path.slice(0, index) : path;
}

function parseAiUnderstanding(raw: string): PptUnderstandingDraft {
  const understandingSummary = cleanAiMarkdown(extractSection(raw, "AI理解摘要"));
  return preparePptUnderstandingDraftForDisplay({
    understandingSummary: understandingSummary || cleanAiMarkdown(raw),
    keyPriorities: cleanAiMarkdown(extractSection(raw, "重点取舍")),
    narrativeMainline: cleanAiMarkdown(extractSection(raw, "叙事主线")),
    suggestedPageStructure: cleanAiMarkdown(extractSection(raw, "建议页面结构")),
    visualExpressionAdvice: cleanAiMarkdown(extractSection(raw, "视觉与表达建议")),
    openQuestions: cleanAiMarkdown(extractSection(raw, "仍需确认的问题")),
  });
}

function buildPlanningContext(draft: PptUnderstandingDraft): string {
  return [
    ["AI 理解摘要", draft.understandingSummary],
    ["重点取舍", draft.keyPriorities],
    ["叙事主线", draft.narrativeMainline],
    ["建议页面结构", draft.suggestedPageStructure],
    ["视觉与表达建议", draft.visualExpressionAdvice],
    ["仍需确认的问题", draft.openQuestions],
  ]
    .map(([title, value]) => `## ${title}\n${value}`)
    .join("\n\n");
}

function confirmUnderstandingReplacement(): Promise<boolean> {
  return new Promise((resolve) => {
    Modal.confirm({
      title: "重新理解需求？",
      content: "AI 新结果会覆盖你当前对需求理解的修改。",
      okText: "重新理解",
      cancelText: "保留当前内容",
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    });
  });
}

function confirmMaterialOverwrite(): Promise<boolean> {
  return new Promise((resolve) => {
    Modal.confirm({
      title: "重新导入素材？",
      content: "重新导入将覆盖当前已编辑的合并素材，是否继续？",
      okText: "继续导入",
      cancelText: "保留当前内容",
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    });
  });
}

function confirmChunkedMaterialAnalysis(totalChunks: number): Promise<boolean> {
  return new Promise((resolve) => {
    Modal.confirm({
      title: PPT_UNDERSTANDING_UI_COPY.feeTitle,
      content: (
        <div className="space-y-3">
          <p>{formatPptFeeIntroduction(totalChunks)}</p>
          <p>{PPT_UNDERSTANDING_UI_COPY.feeProtection}</p>
          <p>{PPT_UNDERSTANDING_UI_COPY.feeExplanation}</p>
          <details className="text-sm text-slate-500">
            <summary className="cursor-pointer">{PPT_UNDERSTANDING_UI_COPY.feeDetailsTitle}</summary>
            <div className="mt-2 space-y-1 pl-4">
              <div>分段阅读：预计 {totalChunks} 次</div>
              <div>最终整理：预计 1 次</div>
            </div>
          </details>
        </div>
      ),
      okText: PPT_UNDERSTANDING_UI_COPY.feeConfirm,
      cancelText: PPT_UNDERSTANDING_UI_COPY.feeCancel,
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    });
  });
}

function resolveSmartValuesForBudget(
  values: PptSmartDraftFields,
  sourceMaterial: string,
): FinalSmartValues {
  return {
    topic: values.topic?.trim() ?? "",
    sourceMaterial,
    audience:
      values.audience === "自定义"
        ? values.customAudience?.trim() ?? ""
        : values.audience ?? "",
    pageCount:
      values.pageCount === "自定义"
        ? values.customPageCount
          ? `${values.customPageCount} 页`
          : ""
        : values.pageCount ?? "",
    style:
      values.style === "自定义"
        ? values.customStyle?.trim() ?? ""
        : values.style ?? "",
    extraRequirements: values.extraRequirements?.trim() || "无",
  };
}

function formatTokenCount(value: number | null): string {
  return value === null || !Number.isFinite(value)
    ? "未知"
    : Math.round(value).toLocaleString("zh-CN");
}

function confirmStaleUnderstandingExport(): Promise<boolean> {
  return new Promise((resolve) => {
    Modal.confirm({
      title: "AI 理解可能已过期",
      content: "当前 AI 理解基于修改前的素材。建议重新理解需求后再导出。是否仍导出旧版本？",
      okText: "仍然导出",
      cancelText: "取消",
      onOk: () => resolve(true),
      onCancel: () => resolve(false),
    });
  });
}

function flattenFolderPaths(folders: Folder[]): Map<number, string> {
  const paths = new Map<number, string>();
  const visit = (items: Folder[], prefix: string) => {
    for (const folder of items) {
      const path = prefix ? `${prefix} / ${folder.name}` : folder.name;
      paths.set(folder.id, path);
      visit(folder.children ?? [], path);
    }
  };
  visit(folders, "");
  return paths;
}

function materialSourceTitle(note: Note): string {
  const title = note.title.trim();
  if (title) return title;
  if (note.is_daily && note.daily_date) return `${note.daily_date} 的日记`;
  return note.is_daily ? "未命名日记" : "未命名文档";
}

function sourceTypeForNote(note: Note): PptMaterialSourceType {
  return note.is_daily ? "diary" : "document";
}

function buildMaterialCandidates(notes: Note[], folders: Folder[]): MaterialCandidate[] {
  const folderPaths = flattenFolderPaths(folders);
  const tabState = useTabsStore.getState();
  return notes.map((note) => {
    const dirtyTab = tabState.tabs.find((tab) => tab.id === note.id && tab.dirty);
    const draft = tabState.drafts[note.id];
    const dailyDraft = note.is_daily ? getDailyDraftSnapshot(note.id) : null;
    const dailyHasUnsavedChanges = dailyDraft
      ? dailySnapshotHasUnpersistedContent(dailyDraft)
      : false;
    let disabledReason: string | undefined;
    if (note.is_encrypted) {
      disabledReason = "加密内容请先在编辑器中解锁并另存后再导入";
    } else if (dirtyTab && !draft) {
      disabledReason = "该文档存在未保存修改，请先保存后再作为 PPT 素材使用";
    }
    return {
      id: note.id,
      sourceType: sourceTypeForNote(note),
      title: draft?.title.trim() || dailyDraft?.title.trim() || materialSourceTitle(note),
      updatedAt: note.updated_at,
      wordCount: note.word_count,
      folderId: note.folder_id,
      dailyDate: note.daily_date,
      hasUnsavedChanges: Boolean((dirtyTab && draft) || dailyHasUnsavedChanges),
      note,
      folderPath: note.folder_id ? folderPaths.get(note.folder_id) : undefined,
      selectable: !disabledReason,
      disabledReason,
    };
  });
}

function chooseInitialModelId(models: AiModel[], savedId: string): number | null {
  if (models.length === 0) {
    return null;
  }

  const parsedSavedId = Number(savedId);
  if (Number.isFinite(parsedSavedId) && models.some((model) => model.id === parsedSavedId)) {
    return parsedSavedId;
  }

  const defaultModel = models.find((model) => model.is_default);
  if (defaultModel && defaultModel.provider !== "ollama") {
    return defaultModel.id;
  }

  const firstRemoteModel = models.find((model) => model.provider !== "ollama");
  if (firstRemoteModel) {
    return firstRemoteModel.id;
  }

  return defaultModel?.id ?? models[0].id;
}

export default function PptGenerationPage() {
  const navigate = useNavigate();
  const [advancedForm] = Form.useForm<AdvancedFormValues>();
  const [smartForm] = Form.useForm<SmartFormValues>();
  const [checking, setChecking] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [loadingModels, setLoadingModels] = useState(false);
  const [aiModels, setAiModels] = useState<AiModel[]>([]);
  const [checkResult, setCheckResult] = useState<PptMasterCheckResult | null>(null);
  const [exportResult, setExportResult] = useState<PptMasterExportResult | null>(null);
  const [materialPickerOpen, setMaterialPickerOpen] = useState(false);
  const [materialPickerFilter, setMaterialPickerFilter] = useState<MaterialPickerFilter>("all");
  const [materialSearch, setMaterialSearch] = useState("");
  const [materialCandidates, setMaterialCandidates] = useState<MaterialCandidate[]>([]);
  const [pickerSelectedIds, setPickerSelectedIds] = useState<number[]>([]);
  const [materialLoading, setMaterialLoading] = useState(false);
  const [materialLoadError, setMaterialLoadError] = useState<string | null>(null);
  const [understandingExporting, setUnderstandingExporting] = useState(false);
  const [understandingExportError, setUnderstandingExportError] = useState<string | null>(null);
  const [lastUnderstandingExportPath, setLastUnderstandingExportPath] = useState<string | null>(null);

  const activeMode = usePptGenerationDraftStore((state) => state.activeMode);
  const setActiveMode = usePptGenerationDraftStore((state) => state.setActiveMode);
  const smartFields = usePptGenerationDraftStore((state) => state.smartFields);
  const setBasicFields = usePptGenerationDraftStore((state) => state.setBasicFields);
  const selectedModelId = usePptGenerationDraftStore((state) => state.selectedModelId);
  const setSelectedModelId = usePptGenerationDraftStore((state) => state.setSelectedModelId);
  const initializeSelectedModel = usePptGenerationDraftStore((state) => state.initializeSelectedModel);
  const generationMode = usePptGenerationDraftStore((state) => state.generationMode);
  const setGenerationMode = usePptGenerationDraftStore((state) => state.setGenerationMode);
  const blockOnQualityFailure = usePptGenerationDraftStore(
    (state) => state.blockOnQualityFailure,
  );
  const setBlockOnQualityFailure = usePptGenerationDraftStore(
    (state) => state.setBlockOnQualityFailure,
  );
  const outputDir = usePptGenerationDraftStore((state) => state.outputDir);
  const setOutputDir = usePptGenerationDraftStore((state) => state.setOutputDir);
  const initializeOutputDir = usePptGenerationDraftStore((state) => state.initializeOutputDir);
  const materialInputMode = usePptGenerationDraftStore((state) => state.materialInputMode);
  const setMaterialInputMode = usePptGenerationDraftStore((state) => state.setMaterialInputMode);
  const manualRawMaterial = usePptGenerationDraftStore((state) => state.manualRawMaterial);
  const setManualRawMaterial = usePptGenerationDraftStore((state) => state.setManualRawMaterial);
  const resolvedMaterialSources = usePptGenerationDraftStore((state) => state.resolvedMaterialSources);
  const mergedMaterialText = usePptGenerationDraftStore((state) => state.mergedMaterialText);
  const mergedMaterialEdited = usePptGenerationDraftStore((state) => state.mergedMaterialEdited);
  const replaceInternalMaterial = usePptGenerationDraftStore((state) => state.replaceInternalMaterial);
  const setInternalSourcesOnly = usePptGenerationDraftStore((state) => state.setInternalSourcesOnly);
  const setMergedMaterialText = usePptGenerationDraftStore((state) => state.setMergedMaterialText);
  const clearInternalMaterial = usePptGenerationDraftStore((state) => state.clearInternalMaterial);
  const materialRevision = usePptGenerationDraftStore((state) => state.materialRevision);
  const understandingRevision = usePptGenerationDraftStore((state) => state.understandingRevision);
  const materialUnderstandingStale = usePptGenerationDraftStore((state) => state.materialUnderstandingStale);
  const materialProcessingMode = usePptGenerationDraftStore((state) => state.materialProcessingMode);
  const materialChunkPlan = usePptGenerationDraftStore((state) => state.materialChunkPlan);
  const chunkUnderstandingDrafts = usePptGenerationDraftStore((state) => state.chunkUnderstandingDrafts);
  const failedChunkIndexes = usePptGenerationDraftStore((state) => state.failedChunkIndexes);
  const materialAnalysisStatus = usePptGenerationDraftStore((state) => state.materialAnalysisStatus);
  const materialAnalysisProgress = usePptGenerationDraftStore((state) => state.materialAnalysisProgress);
  const materialAnalysisError = usePptGenerationDraftStore((state) => state.materialAnalysisError);
  const beginMaterialAnalysis = usePptGenerationDraftStore((state) => state.beginMaterialAnalysis);
  const setMaterialChunkPlan = usePptGenerationDraftStore((state) => state.setMaterialChunkPlan);
  const setMaterialAnalysisStage = usePptGenerationDraftStore((state) => state.setMaterialAnalysisStage);
  const cacheChunkUnderstandingDraft = usePptGenerationDraftStore((state) => state.cacheChunkUnderstandingDraft);
  const setMaterialAnalysisError = usePptGenerationDraftStore((state) => state.setMaterialAnalysisError);
  const finishMaterialAnalysis = usePptGenerationDraftStore((state) => state.finishMaterialAnalysis);
  const cancelMaterialAnalysis = usePptGenerationDraftStore((state) => state.cancelMaterialAnalysis);
  const understandingDraft = usePptGenerationDraftStore((state) => state.understandingDraft);
  const understandingDraftDirty = usePptGenerationDraftStore((state) => state.understandingDraftDirty);
  const understandingStatus = usePptGenerationDraftStore((state) => state.understandingStatus);
  const understandingError = usePptGenerationDraftStore((state) => state.understandingError);
  const setUnderstandingDraft = usePptGenerationDraftStore((state) => state.setUnderstandingDraft);
  const updateUnderstandingField = usePptGenerationDraftStore((state) => state.updateUnderstandingField);
  const setUnderstandingStatus = usePptGenerationDraftStore((state) => state.setUnderstandingStatus);
  const generationStatus = usePptGenerationDraftStore((state) => state.generationStatus);
  const generationResult = usePptGenerationDraftStore((state) => state.generationResult);
  const generationError = usePptGenerationDraftStore((state) => state.generationError);
  const setGenerationStatus = usePptGenerationDraftStore((state) => state.setGenerationStatus);
  const setGenerationResult = usePptGenerationDraftStore((state) => state.setGenerationResult);
  const setGenerationError = usePptGenerationDraftStore((state) => state.setGenerationError);
  const currentStep = usePptGenerationDraftStore((state) => state.activeStep);
  const resetPptDraft = usePptGenerationDraftStore((state) => state.resetPptDraft);
  const understanding = understandingStatus === "loading";
  const materialAnalysisBusy = ["planning", "analyzing", "merging"].includes(
    materialAnalysisStatus,
  );
  const generating = generationStatus === "loading";
  const selectedAudience = Form.useWatch("audience", smartForm);
  const selectedPageCount = Form.useWatch("pageCount", smartForm);
  const selectedStyle = Form.useWatch("style", smartForm);
  const watchedPptMasterRoot = Form.useWatch("pptMasterRoot", advancedForm);
  const watchedPythonPath = Form.useWatch("pythonPath", advancedForm);
  const watchedProjectPath = Form.useWatch("projectPath", advancedForm);

  useEffect(() => {
    const draft = usePptGenerationDraftStore.getState();
    smartForm.setFieldsValue(draft.smartFields);
    console.info("[PPT Draft Restored]", {
      materialMode: draft.materialInputMode,
      sourceCount: draft.resolvedMaterialSources.length,
      materialLength: getEffectivePptRawMaterial(draft).length,
      hasUnderstanding: Boolean(draft.understandingDraft),
      materialRevision: draft.materialRevision,
      understandingRevision: draft.understandingRevision,
      hasGenerationResult: Boolean(draft.generationResult),
    });
  }, [smartForm]);

  useEffect(() => {
    void (async () => {
      const [pptMasterRoot, pythonPath, projectPath, savedOutputDir] = await Promise.all([
        getConfigOrEmpty(CONFIG_KEYS.pptMasterRoot),
        getConfigOrEmpty(CONFIG_KEYS.pythonPath),
        getConfigOrEmpty(CONFIG_KEYS.defaultProjectPath),
        getConfigOrEmpty(CONFIG_KEYS.outputDir),
      ]);
      advancedForm.setFieldsValue({ pptMasterRoot, pythonPath, projectPath });
      initializeOutputDir(savedOutputDir);
    })();
  }, [advancedForm, initializeOutputDir]);

  useEffect(() => {
    void (async () => {
      setLoadingModels(true);
      try {
        const [models, savedModelId] = await Promise.all([
          aiModelApi.list(),
          getConfigOrEmpty(CONFIG_KEYS.aiModelId),
        ]);
        setAiModels(models);
        if (models.length > 0) {
          initializeSelectedModel(chooseInitialModelId(models, savedModelId));
        }
      } catch (e) {
        message.error(`加载 AI 模型失败: ${e}`);
      } finally {
        setLoadingModels(false);
      }
    })();
  }, [initializeSelectedModel]);

  const canOpenOutput = !!exportResult?.outputPath;
  const selectedModel = useMemo(
    () => aiModels.find((model) => model.id === selectedModelId) ?? null,
    [aiModels, selectedModelId],
  );
  const engineConfigured = !!watchedPptMasterRoot?.trim() && !!watchedPythonPath?.trim();
  const selectedDebugProjectIsExample = isLinHuiyinExampleProject(watchedProjectPath);
  const effectiveRawMaterial = materialInputMode === "internal" ? mergedMaterialText : manualRawMaterial;
  const budgetPromptValues = useMemo(
    () => resolveSmartValuesForBudget(smartFields, effectiveRawMaterial),
    [effectiveRawMaterial, smartFields],
  );
  const understandingPromptParts = useMemo(
    () => buildAiUnderstandingPromptParts(budgetPromptValues),
    [budgetPromptValues],
  );
  const reservedOutputTokens = resolvePptReservedOutputTokens(selectedModel?.max_output_tokens);
  const contextBudget = useMemo(
    () => calculatePptContextBudget({
      modelMaxContextTokens: selectedModel?.max_context ?? null,
      rawMaterial: understandingPromptParts.rawMaterial,
      promptText: understandingPromptParts.promptText,
      metadataText: understandingPromptParts.metadataText,
      reservedOutputTokens,
    }),
    [reservedOutputTokens, selectedModel?.max_context, understandingPromptParts],
  );
  const filteredMaterialCandidates = useMemo(() => {
    const keyword = materialSearch.trim().toLowerCase();
    return materialCandidates.filter((candidate) => {
      if (materialPickerFilter !== "all" && candidate.sourceType !== materialPickerFilter) {
        return false;
      }
      return !keyword || candidate.title.toLowerCase().includes(keyword);
    });
  }, [materialCandidates, materialPickerFilter, materialSearch]);
  const status = useMemo(() => {
    if (exportResult) {
      return exportResult.success ? "success" : "error";
    }
    if (checkResult) {
      return checkResult.ok ? "success" : "warning";
    }
    return "info";
  }, [checkResult, exportResult]);
  const generationPptPath = generationResult?.finalPptxPath || generationResult?.pptxPath || null;
  const generationOutputFolder = getDisplayFolderLabel(generationPptPath);

  useEffect(() => {
    console.info("[PPT Context Budget]", {
      modelId: selectedModelId,
      maxContextTokens: contextBudget.maxContextTokens,
      estimatedMaterialTokens: contextBudget.estimatedMaterialTokens,
      estimatedPromptTokens: contextBudget.estimatedPromptTokens,
      estimatedMetadataTokens: contextBudget.estimatedMetadataTokens,
      estimatedInputTokens: contextBudget.estimatedInputTokens,
      reservedOutputTokens: contextBudget.reservedOutputTokens,
      effectiveInputBudget: contextBudget.effectiveInputBudget,
      remainingTokens: contextBudget.remainingTokens,
      status: contextBudget.status,
    });
  }, [contextBudget, selectedModelId]);

  async function saveConfig(values: AdvancedFormValues) {
    await Promise.all([
      configApi.set(CONFIG_KEYS.pptMasterRoot, values.pptMasterRoot.trim()),
      configApi.set(CONFIG_KEYS.pythonPath, values.pythonPath.trim()),
      configApi.set(CONFIG_KEYS.defaultProjectPath, values.projectPath.trim()),
    ]);
  }

  async function pickDirectory(field: keyof Pick<AdvancedFormValues, "pptMasterRoot" | "projectPath">) {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") {
      advancedForm.setFieldValue(field, picked);
    }
  }

  async function pickPython() {
    const picked = await openDialog({
      multiple: false,
      filters: [{ name: "Python", extensions: ["exe", "cmd", "bat"] }],
    });
    if (typeof picked === "string") {
      advancedForm.setFieldValue("pythonPath", picked);
    }
  }

  async function pickOutputDir() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") {
      setOutputDir(picked);
      await configApi.set(CONFIG_KEYS.outputDir, picked);
    }
  }

  async function handleModelChange(modelId: number) {
    setSelectedModelId(modelId);
    try {
      await configApi.set(CONFIG_KEYS.aiModelId, String(modelId));
    } catch (e) {
      message.warning(`保存 PPT AI 模型选择失败: ${e}`);
    }
  }

  function handleMaterialModeChange(mode: PptMaterialInputMode) {
    setMaterialInputMode(mode);
  }

  async function loadMaterialCandidates() {
    setMaterialLoading(true);
    setMaterialLoadError(null);
    try {
      const notes: Note[] = [];
      let page = 1;
      while (true) {
        const result = await noteApi.list({ page, page_size: 100 });
        notes.push(...result.items);
        if (notes.length >= result.total || result.items.length === 0) break;
        page += 1;
      }
      const folders = await folderApi.list();
      setMaterialCandidates(buildMaterialCandidates(notes, folders));
    } catch (error) {
      const detail = `加载文档和日记失败：${String(error)}`;
      setMaterialLoadError(detail);
      message.error(detail);
    } finally {
      setMaterialLoading(false);
    }
  }

  async function openMaterialPicker() {
    setPickerSelectedIds(resolvedMaterialSources.map((source) => source.id));
    setMaterialPickerOpen(true);
    if (materialCandidates.length === 0) {
      await loadMaterialCandidates();
    } else {
      setMaterialCandidates((current) =>
        buildMaterialCandidates(
          current.map((candidate) => candidate.note),
          [],
        ).map((candidate) => ({
          ...candidate,
          folderPath: current.find((item) => item.id === candidate.id)?.folderPath,
        })),
      );
    }
  }

  function togglePickerSource(candidate: MaterialCandidate, checked: boolean) {
    if (!candidate.selectable) {
      message.warning(candidate.disabledReason);
      return;
    }
    setPickerSelectedIds((current) =>
      checked
        ? current.includes(candidate.id)
          ? current
          : [...current, candidate.id]
        : current.filter((id) => id !== candidate.id),
    );
  }

  async function resolveSelectedMaterialSources(): Promise<ResolvedPptMaterialSource[] | null> {
    const resolved: ResolvedPptMaterialSource[] = [];
    for (const id of pickerSelectedIds) {
      const candidate = materialCandidates.find((item) => item.id === id);
      if (!candidate) {
        message.error(`文档 ${id} 不在当前列表中，请刷新后重试`);
        return null;
      }
      if (!candidate.selectable) {
        message.error(candidate.disabledReason ?? "该内容当前不可读取");
        return null;
      }

      const tabState = useTabsStore.getState();
      const dirtyTab = tabState.tabs.find((tab) => tab.id === id && tab.dirty);
      const draft = tabState.drafts[id];
      const dailyDraft = candidate.sourceType === "diary" ? getDailyDraftSnapshot(id) : null;
      const dailyHasUnsavedChanges = dailyDraft
        ? dailySnapshotHasUnpersistedContent(dailyDraft)
        : false;
      if (dirtyTab && !draft) {
        message.error("该文档存在未保存修改，请先保存后再作为 PPT 素材使用。");
        return null;
      }

      let note: Note;
      try {
        note = await noteApi.get(id);
      } catch (error) {
        message.error(`${candidate.sourceType === "diary" ? "日记" : "文档"}内容读取失败：${String(error)}`);
        return null;
      }
      const content = dirtyTab && draft
        ? draft.content
        : dailyHasUnsavedChanges && dailyDraft
          ? dailyDraft.content
          : note.content;
      const plainText = normalizePptMaterialText(content);
      if (!plainText) {
        message.error(`“${candidate.title}”没有可用正文`);
        return null;
      }
      resolved.push({
        id,
        sourceType: candidate.sourceType,
        title:
          dirtyTab && draft
            ? draft.title.trim() || candidate.title
            : dailyHasUnsavedChanges && dailyDraft
              ? dailyDraft.title.trim() || candidate.title
              : materialSourceTitle(note),
        updatedAt: note.updated_at,
        wordCount: plainText.length,
        folderId: note.folder_id,
        dailyDate: note.daily_date,
        hasUnsavedChanges: Boolean((dirtyTab && draft) || dailyHasUnsavedChanges),
        plainText,
      });
    }
    return resolved;
  }

  async function confirmMaterialSelection() {
    if (pickerSelectedIds.length === 0) {
      message.warning("请至少选择一个文档或日记。");
      return;
    }
    if (mergedMaterialEdited && mergedMaterialText.trim()) {
      const shouldOverwrite = await confirmMaterialOverwrite();
      if (!shouldOverwrite) return;
    }
    setMaterialLoading(true);
    setMaterialLoadError(null);
    try {
      const sources = await resolveSelectedMaterialSources();
      if (!sources) return;
      const merged = mergePptMaterialSources(sources);
      if (!hasSubstantivePptMaterial(merged)) {
        message.error("所选内容没有可用正文。");
        return;
      }
      replaceInternalMaterial(sources, merged, false);
      setMaterialPickerOpen(false);
      console.info("[PPT Material Selection]", {
        mode: "internal",
        documentCount: sources.filter((source) => source.sourceType === "document").length,
        diaryCount: sources.filter((source) => source.sourceType === "diary").length,
        sourceCount: sources.length,
        totalCharacters: merged.length,
        containsUnsavedSource: sources.some((source) => source.hasUnsavedChanges),
      });
    } finally {
      setMaterialLoading(false);
    }
  }

  function rebuildMergedMaterial(sources: ResolvedPptMaterialSource[]) {
    replaceInternalMaterial(sources, mergePptMaterialSources(sources), false);
  }

  function removeMaterialSource(sourceId: number) {
    const next = resolvedMaterialSources.filter((source) => source.id !== sourceId);
    if (!mergedMaterialEdited) {
      rebuildMergedMaterial(next);
      return;
    }
    Modal.confirm({
      title: "删除素材来源",
      content: "是否根据当前来源重新生成合并素材？选择保留编辑内容时，只移除来源记录。",
      okText: "重新生成",
      cancelText: "保留编辑内容",
      onOk: () => rebuildMergedMaterial(next),
      onCancel: () => setInternalSourcesOnly(next),
    });
  }

  function clearMaterialSources() {
    const clear = () => {
      setPickerSelectedIds([]);
      clearInternalMaterial();
    };
    if (!mergedMaterialEdited || !mergedMaterialText.trim()) {
      clear();
      return;
    }
    Modal.confirm({
      title: "清空全部素材？",
      content: "当前合并素材包含手动修改，清空后无法恢复。",
      okText: "清空",
      okButtonProps: { danger: true },
      cancelText: "取消",
      onOk: clear,
    });
  }

  function resolveSmartValues(values: SmartFormValues): FinalSmartValues | null {
    const topic = values.topic?.trim();
    const sourceMaterial = getEffectivePptRawMaterial(
      usePptGenerationDraftStore.getState(),
    );
    const audience =
      values.audience === "自定义" ? values.customAudience?.trim() : values.audience;
    const pageCount =
      values.pageCount === "自定义"
        ? values.customPageCount
          ? `${values.customPageCount} 页`
          : ""
        : values.pageCount;
    const style = values.style === "自定义" ? values.customStyle?.trim() : values.style;

    if (!topic || !sourceMaterial.trim() || !audience || !pageCount || !style) {
      message.warning("请完整填写主题、素材、汇报对象、页数和风格");
      return null;
    }

    return {
      topic,
      sourceMaterial,
      audience,
      pageCount,
      style,
      extraRequirements: values.extraRequirements?.trim() || "无",
    };
  }

  function getCurrentSlideCount(values: SmartFormValues): number | undefined {
    if (values.pageCount === "自定义") {
      return values.customPageCount;
    }
    const match = values.pageCount?.match(/\d+/);
    return match ? Number(match[0]) : undefined;
  }

  function getCurrentStyle(values: SmartFormValues): string | undefined {
    return values.style === "自定义" ? values.customStyle?.trim() : values.style;
  }

  function getCurrentStylePreset(values: SmartFormValues) {
    const style = getCurrentStyle(values);
    return style ? stylePresets[style] : undefined;
  }

  function materialAnalysisIsCurrent(requestedMaterialRevision: number, runId: number): boolean {
    const state = usePptGenerationDraftStore.getState();
    return (
      state.materialRevision === requestedMaterialRevision &&
      state.materialAnalysisRunId === runId
    );
  }

  function chunkUnderstandingContext(finalValues: FinalSmartValues): PptChunkUnderstandingContext {
    return {
      topic: finalValues.topic,
      audience: finalValues.audience,
      pageCount: finalValues.pageCount,
      style: finalValues.style,
      extraRequirements: finalValues.extraRequirements,
    };
  }

  async function executeChunkedMaterialUnderstanding(
    finalValues: FinalSmartValues,
    plan: PptMaterialChunkPlan,
    requestedMaterialRevision: number,
    runId: number,
  ): Promise<void> {
    const context = chunkUnderstandingContext(finalValues);
    const cachedDrafts = usePptGenerationDraftStore.getState().chunkUnderstandingDrafts;
    const pendingCount = plan.chunks.filter(
      (chunk) => !cachedDrafts.some((draft) => draft.chunkId === chunk.id),
    ).length;
    console.info("[PPT Understanding]", {
      mode: "chunked",
      materialRevision: requestedMaterialRevision,
      chunkCount: plan.chunks.length,
      expectedRequestCount: pendingCount + 1,
    });
    try {
      const result = await executePptChunkUnderstandingWorkflow({
        chunks: plan.chunks,
        cachedDrafts,
        isCancelled: () => !materialAnalysisIsCurrent(requestedMaterialRevision, runId),
        analyzeChunk: async (chunk) => {
          const raw = await aiWriteApi.understandPptChunk({
            prompt: buildPptChunkUnderstandingPrompt(context, chunk),
            modelId: selectedModelId!,
          });
          return parsePptChunkUnderstandingResponse(raw, chunk);
        },
        mergeDrafts: async (drafts) => {
          const prompt = buildPptUnderstandingMergePrompt({
            ...context,
            chunks: plan.chunks.map((chunk) => ({
              chunkId: chunk.id,
              chunkIndex: chunk.index,
              sourceTitles: chunk.sourceTitles,
              headingContext: chunk.headingContext,
              draft: drafts.find((draft) => draft.chunkId === chunk.id)!,
            })),
          });
          const raw = await aiWriteApi.mergePptUnderstanding({
            prompt,
            modelId: selectedModelId!,
          });
          return parsePptUnderstandingMergeResponse(raw);
        },
        onChunkStarted: (chunk) => {
          setMaterialAnalysisStage(
            "analyzing",
            { current: chunk.index, total: plan.chunks.length, stage: "analyzing" },
            requestedMaterialRevision,
            runId,
          );
          console.info("[PPT Chunk Understanding]", {
            chunkIndex: chunk.index,
            status: "started",
          });
        },
        onChunkSucceeded: (draft) => {
          cacheChunkUnderstandingDraft(draft, requestedMaterialRevision, runId);
          console.info("[PPT Chunk Understanding]", {
            chunkIndex: draft.chunkIndex,
            status: "success",
          });
        },
        onChunkFailed: (chunk) => {
          console.info("[PPT Chunk Understanding]", {
            chunkIndex: chunk.index,
            status: "failed",
          });
        },
        onMergeStarted: () => {
          setMaterialAnalysisStage(
            "merging",
            { current: plan.chunks.length, total: plan.chunks.length, stage: "merging" },
            requestedMaterialRevision,
            runId,
          );
          console.info("[PPT Understanding Merge]", {
            chunkCount: plan.chunks.length,
            status: "started",
          });
        },
      });
      if (!materialAnalysisIsCurrent(requestedMaterialRevision, runId) || result.cancelled) return;
      if (result.failedChunkIndexes.length > 0 || !result.finalDraft) {
        const indexes = result.failedChunkIndexes.length > 0
          ? result.failedChunkIndexes
          : plan.chunks
              .filter((chunk) => !result.drafts.some((draft) => draft.chunkId === chunk.id))
              .map((chunk) => chunk.index);
        const detail = formatPptFailedParts(indexes);
        setMaterialAnalysisError(detail, indexes, requestedMaterialRevision, runId);
        setUnderstandingStatus("error", detail);
        message.error(detail);
        return;
      }
      setUnderstandingDraft(result.finalDraft, requestedMaterialRevision);
      finishMaterialAnalysis(requestedMaterialRevision, runId);
      console.info("[PPT Understanding Merge]", {
        chunkCount: plan.chunks.length,
        status: "success",
      });
      message.success(PPT_UNDERSTANDING_UI_COPY.success);
    } catch (error) {
      if (!materialAnalysisIsCurrent(requestedMaterialRevision, runId)) return;
      const messageText = `最终整理未能完成：${String(error)}`;
      setMaterialAnalysisError(messageText, [], requestedMaterialRevision, runId);
      setUnderstandingStatus("error", messageText);
      console.info("[PPT Understanding Merge]", {
        chunkCount: plan.chunks.length,
        status: "failed",
      });
      message.error(messageText);
    }
  }

  async function executeDirectMaterialUnderstanding(
    finalValues: FinalSmartValues,
    requestedMaterialRevision: number,
  ): Promise<void> {
    const runId = beginMaterialAnalysis("direct", requestedMaterialRevision);
    setUnderstandingStatus("loading");
    console.info("[PPT Understanding]", {
      mode: "direct",
      materialRevision: requestedMaterialRevision,
      chunkCount: 0,
      expectedRequestCount: 1,
    });
    try {
      const raw = await aiWriteApi.understandPpt({
        prompt: buildAiUnderstandingPrompt(finalValues),
        modelId: selectedModelId,
      });
      if (!materialAnalysisIsCurrent(requestedMaterialRevision, runId)) return;
      const parsed = parseAiUnderstanding(raw);
      setUnderstandingDraft(parsed, requestedMaterialRevision);
      finishMaterialAnalysis(requestedMaterialRevision, runId);
      message.success("AI 已完成需求理解");
    } catch (error) {
      if (!materialAnalysisIsCurrent(requestedMaterialRevision, runId)) return;
      const messageText = String(error);
      setMaterialAnalysisError(messageText, [], requestedMaterialRevision, runId);
      setUnderstandingStatus("error", messageText);
      message.error(messageText);
    }
  }

  async function handleUnderstandNeeds() {
    if (!selectedModelId) {
      message.warning("尚未配置 AI 模型，请先到设置页添加模型。");
      return;
    }

    if (materialInputMode === "internal") {
      if (resolvedMaterialSources.length === 0) {
        message.warning("请至少选择一个文档或日记。");
        return;
      }
      if (materialLoadError) {
        message.error(materialLoadError);
        return;
      }
      if (!hasSubstantivePptMaterial(mergedMaterialText)) {
        message.warning("所选文档没有可用正文。");
        return;
      }
    }

    const values = await smartForm.validateFields();
    const finalValues = resolveSmartValues(values);
    if (!finalValues) {
      return;
    }
    if (understandingDraft && understandingDraftDirty) {
      const shouldReplace = await confirmUnderstandingReplacement();
      if (!shouldReplace) {
        return;
      }
    }

    const requestedMaterialRevision = usePptGenerationDraftStore.getState().materialRevision;
    const livePromptParts = buildAiUnderstandingPromptParts(finalValues);
    const liveBudget = calculatePptContextBudget({
      modelMaxContextTokens: selectedModel?.max_context ?? null,
      rawMaterial: livePromptParts.rawMaterial,
      promptText: livePromptParts.promptText,
      metadataText: livePromptParts.metadataText,
      reservedOutputTokens,
    });
    if (liveBudget.status === "unknown") {
      Modal.confirm({
        title: "需要完善模型设置",
        content: "当前模型信息不完整，暂时无法判断应一次阅读还是分段阅读。请先完善模型配置。",
        okText: "前往模型配置",
        cancelText: "留在当前页",
        onOk: () => navigate("/settings"),
      });
      return;
    }
    const requestPlan = resolvePptMaterialRequestPlan({ contextStatus: liveBudget.status });
    if (requestPlan.mode === "direct") {
      await executeDirectMaterialUnderstanding(finalValues, requestedMaterialRevision);
      return;
    }

    const existing = usePptGenerationDraftStore.getState();
    const canReusePlan =
      existing.chunkAnalysisRevision === requestedMaterialRevision &&
      existing.materialChunkPlan !== null;
    const runId = beginMaterialAnalysis("chunked", requestedMaterialRevision, canReusePlan);
    let plan: PptMaterialChunkPlan;
    try {
      plan = canReusePlan
        ? existing.materialChunkPlan!
        : planPptMaterialChunks({
            rawMaterial: finalValues.sourceMaterial,
            modelMaxContextTokens: selectedModel?.max_context ?? null,
            reservedOutputTokens,
            promptContext: chunkUnderstandingContext(finalValues),
          });
      setMaterialChunkPlan(plan, requestedMaterialRevision, runId);
      const cachedIds = new Set(
        usePptGenerationDraftStore.getState().chunkUnderstandingDrafts.map((draft) => draft.chunkId),
      );
      const chunksToRequest = plan.chunks.filter((chunk) => !cachedIds.has(chunk.id)).length;
      const chunkedRequestPlan = resolvePptMaterialRequestPlan({
        contextStatus: liveBudget.status,
        totalChunks: plan.chunks.length,
        cachedChunks: plan.chunks.length - chunksToRequest,
      });
      console.info("[PPT Understanding]", {
        mode: "chunked",
        materialRevision: requestedMaterialRevision,
        chunkCount: plan.chunks.length,
        expectedRequestCount: chunksToRequest + 1,
      });
      if (
        chunkedRequestPlan.requiresFeeConfirmation &&
        !(await confirmChunkedMaterialAnalysis(plan.chunks.length))
      ) {
        cancelMaterialAnalysis();
        return;
      }
    } catch (error) {
      if (!materialAnalysisIsCurrent(requestedMaterialRevision, runId)) return;
      const messageText = String(error);
      setMaterialAnalysisError(messageText, [], requestedMaterialRevision, runId);
      setUnderstandingStatus("error", messageText);
      message.error(messageText);
      return;
    }
    setUnderstandingStatus("loading");
    await executeChunkedMaterialUnderstanding(
      finalValues,
      plan,
      requestedMaterialRevision,
      runId,
    );
  }

  function handleCancelMaterialAnalysis() {
    cancelMaterialAnalysis();
    setUnderstandingStatus("idle");
    message.info("已停止分析。已经完成的部分会被保留，下次可以继续。");
  }

  function handleClearSmartForm() {
    Modal.confirm({
      title: "清空当前 PPT 草稿？",
      content: "这将清除当前素材、AI 理解和生成结果，是否继续？",
      okText: "清空草稿",
      okButtonProps: { danger: true },
      cancelText: "取消",
      onOk: () => {
        resetPptDraft();
        const reset = usePptGenerationDraftStore.getState();
        smartForm.setFieldsValue(reset.smartFields);
        setPickerSelectedIds([]);
        setMaterialLoadError(null);
        console.info("[PPT Draft Reset]", { reason: "user_confirmed" });
      },
    });
  }

  function updateUnderstandingDraft(field: keyof PptUnderstandingDraft, value: string) {
    updateUnderstandingField(field, value);
  }

  async function handleExportUnderstandingMarkdown() {
    const draftState = usePptGenerationDraftStore.getState();
    const draft = draftState.understandingDraft;
    if (!draft) {
      message.warning("请先完成 AI 需求理解");
      return;
    }

    const stale = draftState.understandingRevision !== draftState.materialRevision;
    if (stale && !(await confirmStaleUnderstandingExport())) {
      console.info("[PPT Understanding Export]", { status: "cancelled" });
      return;
    }

    const fields = draftState.smartFields;
    const audience = fields.audience === "自定义"
      ? fields.customAudience?.trim() || "暂无"
      : fields.audience;
    const pageCount = fields.pageCount === "自定义"
      ? fields.customPageCount
        ? `${fields.customPageCount} 页`
        : "暂无"
      : fields.pageCount;
    const style = fields.style === "自定义"
      ? fields.customStyle?.trim() || "暂无"
      : fields.style;
    const output = buildPptUnderstandingMarkdown({
      title: fields.topic,
      audience,
      pageCount,
      style,
      generationMode: draftState.generationMode,
      understandingDraft: draft,
      materialSources: draftState.resolvedMaterialSources,
      materialInputMode: draftState.materialInputMode,
      exportedAt: new Date(),
      stale,
    });

    console.info("[PPT Understanding Export]", {
      status: "started",
      summaryLength: draft.understandingSummary.length,
      prioritiesLength: draft.keyPriorities.length,
      narrativeLength: draft.narrativeMainline.length,
      pageStructureLength: draft.suggestedPageStructure.length,
      visualAdviceLength: draft.visualExpressionAdvice.length,
      openQuestionsLength: draft.openQuestions.length,
      sourceCount: draftState.resolvedMaterialSources.length,
      stale,
    });

    setUnderstandingExporting(true);
    setUnderstandingExportError(null);
    try {
      const path = await saveDialog({
        defaultPath: output.filename,
        filters: [{ name: "Markdown 文件", extensions: ["md"] }],
      });
      if (typeof path !== "string" || !path.trim()) {
        console.info("[PPT Understanding Export]", { status: "cancelled" });
        return;
      }
      await systemApi.writeTextFile(path, output.content);
      setLastUnderstandingExportPath(path);
      console.info("[PPT Understanding Export]", { status: "success", path });
      message.success(`AI 理解已导出：${path}`);
    } catch (error) {
      const detail = `无法写入所选文件：${String(error)}`;
      setUnderstandingExportError(detail);
      console.error("[PPT Understanding Export]", { status: "failed", reason: String(error) });
      message.error(detail);
    } finally {
      setUnderstandingExporting(false);
    }
  }

  async function handleConfirmGenerate() {
    console.info("[PPT UI] generate button clicked");
    const draft = understandingDraft;
    if (!draft) {
      const error = materialUnderstandingStale
        ? "素材已变化，请重新理解需求"
        : "请先让 AI 理解需求，再确认生成";
      console.warn("[PPT UI] generate validation failed", error);
      setGenerationError(error);
      message.warning(error);
      return;
    }
    if (understandingRevision !== usePptGenerationDraftStore.getState().materialRevision) {
      const error = "素材已变化，请重新理解需求";
      console.warn("[PPT UI] generate validation failed", error, {
        materialRevision,
        understandingRevision,
      });
      setGenerationError(error);
      message.warning(error);
      return;
    }
    const missingRequiredFields = [
      ["AI 理解摘要", draft.understandingSummary],
      ["叙事主线", draft.narrativeMainline],
      ["建议页面结构", draft.suggestedPageStructure],
    ]
      .filter(([, value]) => !value.trim())
      .map(([label]) => label);
    if (missingRequiredFields.length > 0) {
      const error = `请补充以下必填理解项：${missingRequiredFields.join("、")}`;
      console.warn("[PPT UI] generate validation failed", error);
      setGenerationError(error);
      message.warning(error);
      return;
    }
    if (!selectedModelId) {
      const error = "未选择 AI 模型，请先选择可用模型";
      console.warn("[PPT UI] generate validation failed", error);
      setGenerationError(error);
      message.warning(error);
      return;
    }

    const advancedValues = advancedForm.getFieldsValue();
    const [savedPptMasterRoot, savedPythonPath] = await Promise.all([
      getConfigOrEmpty(CONFIG_KEYS.pptMasterRoot),
      getConfigOrEmpty(CONFIG_KEYS.pythonPath),
    ]);
    const pptMasterRoot = advancedValues.pptMasterRoot?.trim() || savedPptMasterRoot.trim();
    const pythonPath = advancedValues.pythonPath?.trim() || savedPythonPath.trim();
    if (!pptMasterRoot || !pythonPath) {
      const error = !pptMasterRoot ? "缺少 ppt-master 根目录" : "缺少 Python 路径";
      console.warn("[PPT UI] generate validation failed", error, {
        hasPptMasterRoot: Boolean(pptMasterRoot),
        hasPythonPath: Boolean(pythonPath),
      });
      setGenerationError(error);
      message.error("生成引擎尚未配置。开发阶段请先到“开发者调试”填写 ppt-master 根目录和 Python 路径；打包版本将内置生成引擎。");
      return;
    }

    const smartValues = smartForm.getFieldsValue();
    const finalSmartValues = resolveSmartValues(smartValues);
    if (!finalSmartValues) {
      const error = "请完整填写 PPT 基础信息";
      setGenerationError(error);
      return;
    }
    const sourceMaterial = finalSmartValues.sourceMaterial;
    const extraRequirements = finalSmartValues.extraRequirements;
    const stylePreset = getCurrentStylePreset(smartValues);
    const generationEngine = generationMode === "agent" ? "ppt_master_native" : "legacy_fallback";
    if (!["ppt_master_native", "legacy_fallback"].includes(generationEngine)) {
      const error = `generationEngine 无效: ${generationEngine}`;
      console.warn("[PPT UI] generate validation failed", error);
      setGenerationError(error);
      message.error(error);
      return;
    }
    const planningContext = buildPlanningContext(draft);
    const payload: PptMasterGenerateInput = {
      pptMasterRoot,
      pythonPath,
      prompt: finalSmartValues.topic,
      planningContext,
      aiUnderstandingResult: draft,
      understandingSummary: draft.understandingSummary,
      keyPriorities: draft.keyPriorities,
      suggestedPageStructure: draft.suggestedPageStructure,
      narrativeMainline: draft.narrativeMainline,
      visualExpressionAdvice: draft.visualExpressionAdvice,
      visualSuggestions: draft.visualExpressionAdvice,
      openQuestions: draft.openQuestions,
      rawMaterial: sourceMaterial,
      materialSources:
        materialInputMode === "internal"
          ? resolvedMaterialSources.map((source) => ({
              id: source.id,
              sourceType: source.sourceType,
              title: source.title,
            }))
          : [],
      extraRequirements,
      modelId: selectedModelId,
      title: finalSmartValues.topic,
      audience: finalSmartValues.audience,
      slideCount: getCurrentSlideCount(smartValues),
      style: finalSmartValues.style,
      customStyle:
        smartValues.style === "自定义" ? smartValues.customStyle?.trim() || null : null,
      generationEngine,
      mode: stylePreset?.mode,
      visualStyle: stylePreset?.visualStyle,
      layoutBias: stylePreset?.layoutBias,
      chartBias: stylePreset?.chartBias,
      outputDir: outputDir.trim() || null,
      generationMode,
      blockOnQualityFailure:
        generationMode === "agent" ? blockOnQualityFailure : undefined,
    };
    console.info("[PPT Understanding Confirmed]", {
      summaryLength: draft.understandingSummary.length,
      prioritiesLength: draft.keyPriorities.length,
      narrativeLength: draft.narrativeMainline.length,
      pageStructureLength: draft.suggestedPageStructure.length,
      visualAdviceLength: draft.visualExpressionAdvice.length,
      openQuestionsLength: draft.openQuestions.length,
      rawMaterialLength: sourceMaterial.length,
    });
    setGenerationResult(null);
    setGenerationStatus("loading");
    setExportResult(null);
    setCheckResult(null);
    try {
      console.info("[PPT UI] before generateFromPrompt invoke", {
        generationEngine,
        generationMode,
        blockOnQualityFailure: payload.blockOnQualityFailure,
        slideCount: payload.slideCount,
        style: payload.style,
        hasOutputDir: Boolean(payload.outputDir),
      });
      const result = await pptMasterApi.generateFromPrompt(payload);
      console.info("[PPT UI] generateFromPrompt resolved", {
        success: result.success,
        generationEngine: result.generationEngine,
        durationMs: result.durationMs,
      });
      setGenerationResult(result);
      if (result.success) {
        message.success("PPTX 生成完成");
      } else {
        message.error(result.error ?? "PPTX 生成失败");
      }
    } catch (e) {
      console.error("[PPT UI] generateFromPrompt failed", e);
      const error = String(e);
      message.error(error);
      setGenerationResult({
        success: false,
        projectPath: null,
        pptxPath: null,
        finalPptxPath: null,
        slidePlanPath: null,
        designSpecPath: null,
        qualityCheckPassed: null,
        generationMode,
        exitCode: null,
        stdout: "",
        stderr: "",
        durationMs: 0,
        error,
      });
    }
  }

  async function handleCheck() {
    const values = await advancedForm.validateFields();
    setChecking(true);
    setCheckResult(null);
    setExportResult(null);
    try {
      await saveConfig(values);
      const result = await pptMasterApi.check({
        pptMasterRoot: values.pptMasterRoot.trim(),
        pythonPath: values.pythonPath.trim(),
      });
      setCheckResult(result);
      if (result.ok) {
        message.success("环境检测通过");
      } else {
        message.warning("环境检测未通过");
      }
    } catch (e) {
      message.error(String(e));
    } finally {
      setChecking(false);
    }
  }

  async function handleExport() {
    const values = await advancedForm.validateFields();
    setExporting(true);
    setExportResult(null);
    try {
      await saveConfig(values);
      const result = await pptMasterApi.export({
        pptMasterRoot: values.pptMasterRoot.trim(),
        pythonPath: values.pythonPath.trim(),
        projectPath: values.projectPath.trim(),
      });
      setExportResult(result);
      if (result.success) {
        message.success("PPTX 导出完成");
      } else {
        message.error(result.error ?? "PPTX 导出失败");
      }
    } catch (e) {
      message.error(String(e));
    } finally {
      setExporting(false);
    }
  }

  const smartGenerate = (
    <div className="space-y-4">
      <Alert
        type="info"
        showIcon
        message="当前版本使用内置模板式生成流程，适合快速生成初版 PPT；后续可接入更复杂的设计 Agent 进行美化。"
      />

      {!engineConfigured && (
        <Alert
          type="warning"
          showIcon
          message="生成引擎尚未配置，请先到“开发者调试”中完成本地引擎配置。后续打包版本将内置生成引擎，无需手动配置。"
        />
      )}

      <Card>
        <Form
          form={smartForm}
          layout="vertical"
          onValuesChange={(changedValues) => {
            setBasicFields(changedValues);
          }}
          initialValues={smartFields}
        >
          <Form.Item
            label="AI 模型"
            extra="不同任务适合不同模型。PPT 需求理解建议选择远程 OpenAI-compatible、DeepSeek、OpenRouter、SiliconFlow 等模型；不建议默认使用本地 Ollama。"
          >
            <Select
              value={selectedModelId ?? undefined}
              loading={loadingModels}
              placeholder="请选择 AI 模型"
              optionFilterProp="label"
              showSearch
              onChange={handleModelChange}
              options={aiModels.map((model) => ({
                value: model.id,
                label: `${model.name} · ${model.provider} · ${model.model_id}`,
              }))}
            />
          </Form.Item>

          {aiModels.length === 0 && (
            <Alert
              className="mb-4"
              type="warning"
              showIcon
              message="尚未配置 AI 模型，请先到设置页添加 OpenAI-compatible 模型。"
            />
          )}

          {selectedModel?.provider === "ollama" && (
            <Alert
              className="mb-4"
              type="warning"
              showIcon
              message="当前选择的是本地 Ollama。如需使用远程 API，请到设置页添加 OpenAI-compatible 模型。"
            />
          )}

          <Form.Item
            label="导出文件夹"
            extra="可选择最终 PPTX 保存位置；不选择时会保存在自动生成项目的 exports 文件夹中。"
          >
            <Input
              value={outputDir}
              placeholder="请选择最终 PPTX 保存文件夹（可选）"
              onChange={(e) => setOutputDir(e.target.value)}
              onBlur={() => configApi.set(CONFIG_KEYS.outputDir, outputDir.trim()).catch(() => {})}
              addonAfter={
                <Button type="text" size="small" icon={<FolderOpen size={14} />} onClick={pickOutputDir} />
              }
            />
          </Form.Item>

          <Form.Item
            name="topic"
            label="PPT 主题"
            rules={[{ required: true, message: "请输入 PPT 主题" }]}
          >
            <Input placeholder="例如：课程主题、汇报题目或教学内容" />
          </Form.Item>

          <Form.Item label="素材来源">
            <Segmented
              value={materialInputMode}
              onChange={(value) => handleMaterialModeChange(value as PptMaterialInputMode)}
              options={[
                { label: "直接输入", value: "manual" },
                { label: "软件内文档", value: "internal" },
              ]}
            />
            <div className="mt-2 text-xs text-slate-500">
              {materialInputMode === "manual"
                ? "粘贴或输入文字材料。"
                : "从 Pomegranate 的文档和日记中选择内容。"}
            </div>
          </Form.Item>

          {materialInputMode === "manual" ? (
            <Form.Item
              label="原始语料"
              required
            >
              <Input.TextArea
                value={manualRawMaterial}
                onChange={(event) => setManualRawMaterial(event.target.value)}
                autoSize={{ minRows: 8, maxRows: 16 }}
                placeholder="粘贴笔记、文档摘要、项目说明、实验内容等"
              />
            </Form.Item>
          ) : (
            <div className="mb-6 space-y-3 rounded-md border border-slate-200 p-4">
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <div className="font-medium text-slate-900">软件内文档素材</div>
                  <div className="mt-1 text-xs text-slate-500">
                    已选择 {resolvedMaterialSources.length} 项，共 {mergedMaterialText.length} 字符
                  </div>
                </div>
                <Space wrap>
                  <Button icon={<Search size={15} />} onClick={openMaterialPicker}>
                    选择文档或日记
                  </Button>
                  <Button
                    icon={<Trash2 size={15} />}
                    disabled={resolvedMaterialSources.length === 0 && !mergedMaterialText}
                    onClick={clearMaterialSources}
                  >
                    清空选择
                  </Button>
                </Space>
              </div>

              {materialLoadError && <Alert type="error" showIcon message={materialLoadError} />}

              {resolvedMaterialSources.length > 0 ? (
                <List
                  size="small"
                  dataSource={resolvedMaterialSources}
                  renderItem={(source) => (
                    <List.Item
                      actions={[
                        <Button
                          key="remove"
                          type="text"
                          danger
                          icon={<Trash2 size={14} />}
                          title="移除此来源"
                          onClick={() => removeMaterialSource(source.id)}
                        />,
                      ]}
                    >
                      <Space size="small" wrap>
                        <Tag color={source.sourceType === "diary" ? "gold" : "blue"}>
                          {source.sourceType === "diary" ? "日记" : "文档"}
                        </Tag>
                        <Text>{source.title}</Text>
                        {source.hasUnsavedChanges && <Tag color="orange">包含未保存修改</Tag>}
                      </Space>
                    </List.Item>
                  )}
                />
              ) : (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="尚未选择文档或日记" />
              )}

              <div>
                <div className="mb-2 font-medium text-slate-900">已合并 PPT 素材</div>
                <Input.TextArea
                  value={mergedMaterialText}
                  onChange={(event) => {
                    setMergedMaterialText(event.target.value, true);
                  }}
                  autoSize={{ minRows: 10, maxRows: 20 }}
                  placeholder="选择文档或日记后，正文将在这里合并。你可以继续编辑。"
                />
                <div className="mt-2 text-xs text-slate-500">
                  当前共 {mergedMaterialText.length} 字符。用户编辑后的内容将作为最终原始素材。
                </div>
              </div>
            </div>
          )}

          {effectiveRawMaterial.trim() && contextBudget.status === "exceeded" && (
            <Alert
              className="mb-4"
              type="info"
              showIcon
              message={PPT_UNDERSTANDING_UI_COPY.longMaterialTitle}
              description={
                <div className="space-y-1">
                  <div>{PPT_UNDERSTANDING_UI_COPY.longMaterialDescription}</div>
                  <div>{PPT_UNDERSTANDING_UI_COPY.originalMaterialProtection}</div>
                </div>
              }
            />
          )}

          {effectiveRawMaterial.trim() && contextBudget.status === "unknown" && (
            <Alert
              className="mb-4"
              type="warning"
              showIcon
              message="当前模型信息不完整"
              description="暂时无法判断这份材料应一次阅读还是分段阅读，请先完善模型设置。"
            />
          )}

          {effectiveRawMaterial.trim() && (
            <details className="mb-4 rounded-md border border-slate-200 bg-slate-50 px-3 py-2 text-sm">
              <summary className="cursor-pointer text-slate-600">查看技术详情</summary>
              <div className="mt-3 grid gap-x-6 gap-y-1 text-xs text-slate-500 sm:grid-cols-2">
                <span>当前模型：{selectedModel?.name ?? "未选择"}</span>
                <span>最大上下文：{formatTokenCount(contextBudget.maxContextTokens)}</span>
                <span>预计输入 token：{formatTokenCount(contextBudget.estimatedInputTokens)}</span>
                <span>输出预留：{formatTokenCount(contextBudget.reservedOutputTokens)}</span>
                <span>可用输入预算：{formatTokenCount(contextBudget.effectiveInputBudget)}</span>
                <span>分段数量：{materialChunkPlan?.chunks.length ?? (contextBudget.status === "exceeded" ? "开始前计算" : 1)}</span>
                <span>
                  预计模型调用次数：
                  {materialChunkPlan
                    ? materialChunkPlan.chunks.filter(
                        (chunk) => !chunkUnderstandingDrafts.some((draft) => draft.chunkId === chunk.id),
                      ).length + 1
                    : contextBudget.status === "exceeded"
                      ? "开始前计算"
                      : 1}
                </span>
              </div>
            </details>
          )}

          {materialAnalysisBusy && materialAnalysisProgress && (
            <Alert
              className="mb-4"
              type="info"
              showIcon
              message={
                materialAnalysisStatus === "planning"
                  ? PPT_UNDERSTANDING_UI_COPY.progressPlanning
                  : materialAnalysisStatus === "analyzing"
                    ? materialProcessingMode === "direct"
                      ? PPT_UNDERSTANDING_UI_COPY.progressDirect
                      : formatPptAnalysisProgress(
                          materialAnalysisProgress.current,
                          materialAnalysisProgress.total,
                        )
                    : PPT_UNDERSTANDING_UI_COPY.progressMerging
              }
              description={PPT_UNDERSTANDING_UI_COPY.progressDescription}
              action={
                <Button size="small" onClick={handleCancelMaterialAnalysis}>
                  {PPT_UNDERSTANDING_UI_COPY.progressCancel}
                </Button>
              }
            />
          )}

          {materialAnalysisStatus === "success" && materialProcessingMode === "chunked" && (
            <Alert
              className="mb-4"
              type="success"
              showIcon
              message={PPT_UNDERSTANDING_UI_COPY.success}
              description={PPT_UNDERSTANDING_UI_COPY.successDescription}
            />
          )}

          {materialAnalysisStatus === "error" && materialAnalysisError && (
            <Alert
              className="mb-4"
              type="error"
              showIcon
              message={
                failedChunkIndexes.length > 0
                  ? formatPptFailedParts(failedChunkIndexes)
                  : PPT_UNDERSTANDING_UI_COPY.failure
              }
              description={PPT_UNDERSTANDING_UI_COPY.failureDescription}
              action={
                <Button size="small" onClick={handleUnderstandNeeds}>
                  {failedChunkIndexes.length > 0
                    ? PPT_UNDERSTANDING_UI_COPY.retryFailedPart
                    : PPT_UNDERSTANDING_UI_COPY.retry}
                </Button>
              }
            />
          )}

          {materialUnderstandingStale && (
            <Alert
              className="mb-4"
              type="warning"
              showIcon
              message="素材已变化，请重新理解需求"
            />
          )}

          {understandingStatus === "error" && understandingError && !materialAnalysisError && (
            <Alert
              className="mb-4"
              type="error"
              showIcon
              message="AI 理解需求失败"
              description={understandingError}
            />
          )}

          <div className="grid gap-4 md:grid-cols-3">
            <Form.Item name="audience" label="汇报对象" rules={[{ required: true }]}>
              <Select options={audienceOptions.map((value) => ({ value, label: value }))} />
            </Form.Item>

            <Form.Item name="pageCount" label="页数" rules={[{ required: true }]}>
              <Select options={pageCountOptions.map((value) => ({ value, label: value }))} />
            </Form.Item>

            <Form.Item name="style" label="风格" rules={[{ required: true }]}>
              <Select
                options={styleOptions.map((value) => ({
                  value,
                  label: value,
                  title: stylePresets[value]?.description,
                }))}
              />
            </Form.Item>
          </div>

          {(selectedAudience === "自定义" || selectedPageCount === "自定义" || selectedStyle === "自定义") && (
            <div className="grid gap-4 md:grid-cols-3">
              {selectedAudience === "自定义" && (
                <Form.Item
                  name="customAudience"
                  label="自定义汇报对象"
                  rules={[{ required: true, message: "请输入自定义汇报对象" }]}
                >
                  <Input placeholder="例如：校赛评委、创新创业比赛专家、投资人" />
                </Form.Item>
              )}

              {selectedPageCount === "自定义" && (
                <Form.Item
                  name="customPageCount"
                  label="自定义页数"
                  rules={[{ required: true, message: "请输入自定义页数" }]}
                >
                  <InputNumber min={1} max={30} precision={0} className="w-full" placeholder="1-30" />
                </Form.Item>
              )}

              {selectedStyle === "自定义" && (
                <Form.Item
                  name="customStyle"
                  label="自定义风格"
                  rules={[{ required: true, message: "请输入自定义风格" }]}
                >
                  <Input placeholder="例如：蓝白答辩风、简洁课堂风、图文讲解风" />
                </Form.Item>
              )}
            </div>
          )}

          <Form.Item name="extraRequirements" label="额外要求">
            <Input.TextArea
              autoSize={{ minRows: 3, maxRows: 8 }}
              placeholder="例如：突出创新点、少放大段文字、适合答辩展示"
            />
          </Form.Item>

          <Space wrap>
            <Button
              type="primary"
              icon={<Wand2 size={15} />}
              loading={understanding}
              onClick={handleUnderstandNeeds}
            >
              {understanding ? "AI 正在理解需求..." : "理解需求"}
            </Button>
            <Button icon={<RotateCcw size={15} />} onClick={handleClearSmartForm}>
              清空当前 PPT 草稿
            </Button>
          </Space>
        </Form>
      </Card>

      <Modal
        title="选择文档或日记"
        open={materialPickerOpen}
        width={820}
        okText="导入所选内容"
        cancelText="取消"
        confirmLoading={materialLoading}
        onOk={confirmMaterialSelection}
        onCancel={() => setMaterialPickerOpen(false)}
      >
        <div className="mb-4 flex flex-wrap items-center gap-3">
          <Segmented
            value={materialPickerFilter}
            onChange={(value) => setMaterialPickerFilter(value as MaterialPickerFilter)}
            options={[
              { label: "全部", value: "all" },
              { label: "文档", value: "document" },
              { label: "日记", value: "diary" },
            ]}
          />
          <Input
            className="min-w-56 flex-1"
            allowClear
            prefix={<Search size={14} />}
            value={materialSearch}
            onChange={(event) => setMaterialSearch(event.target.value)}
            placeholder="按标题搜索"
          />
          <Button onClick={loadMaterialCandidates} loading={materialLoading}>
            刷新
          </Button>
        </div>

        {materialLoadError && <Alert className="mb-3" type="error" showIcon message={materialLoadError} />}

        <Spin spinning={materialLoading}>
          <div className="max-h-[52vh] overflow-y-auto rounded-md border border-slate-200">
            {filteredMaterialCandidates.length > 0 ? (
              <List
                dataSource={filteredMaterialCandidates}
                renderItem={(candidate) => {
                  const checked = pickerSelectedIds.includes(candidate.id);
                  return (
                    <List.Item
                      className="px-4"
                      onClick={() => candidate.selectable && togglePickerSource(candidate, !checked)}
                    >
                      <div className="flex w-full items-start gap-3">
                        <Checkbox
                          className="mt-1"
                          checked={checked}
                          disabled={!candidate.selectable}
                          onClick={(event) => event.stopPropagation()}
                          onChange={(event) => togglePickerSource(candidate, event.target.checked)}
                        />
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-2">
                            {candidate.sourceType === "diary" ? (
                              <CalendarDays size={15} className="text-amber-600" />
                            ) : (
                              <FileText size={15} className="text-blue-600" />
                            )}
                            <Text strong>{candidate.title}</Text>
                            <Tag color={candidate.sourceType === "diary" ? "gold" : "blue"}>
                              {candidate.sourceType === "diary" ? "日记" : "文档"}
                            </Tag>
                            {candidate.hasUnsavedChanges && <Tag color="orange">包含未保存修改</Tag>}
                          </div>
                          <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 text-xs text-slate-500">
                            <span>最后修改：{candidate.updatedAt?.slice(0, 16).replace("T", " ") || "未知"}</span>
                            <span>字数：{candidate.wordCount ?? 0}</span>
                            {candidate.sourceType === "diary" && candidate.dailyDate && (
                              <span>日期：{candidate.dailyDate}</span>
                            )}
                            {candidate.folderPath && <span>文件夹：{candidate.folderPath}</span>}
                          </div>
                          {candidate.disabledReason && (
                            <div className="mt-1 text-xs text-red-600">{candidate.disabledReason}</div>
                          )}
                        </div>
                      </div>
                    </List.Item>
                  );
                }}
              />
            ) : (
              <Empty className="py-10" image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有匹配的文档或日记" />
            )}
          </div>
        </Spin>
      </Modal>

      {understandingDraft && (
        <div className="space-y-4">
          <Alert
            type="info"
            showIcon
            message="以下内容将直接作为 PPT 的规划依据，你可以逐项修改后再生成。"
          />

          {understandingFieldDefinitions.map((field) => (
            <Card key={field.key} title={`${field.title}${field.required ? " *" : ""}`}>
              <Text type="secondary">{field.description}</Text>
              <Input.TextArea
                className="mt-3"
                value={understandingDraft[field.key]}
                onChange={(event) => updateUnderstandingDraft(field.key, event.target.value)}
                autoSize={{ minRows: field.minRows, maxRows: field.maxRows }}
                aria-label={field.title}
              />
            </Card>
          ))}

          <Card title="生成模式">
            <Radio.Group
              value={generationMode}
              onChange={(event) => setGenerationMode(event.target.value as "agent" | "template")}
            >
              <Space direction="vertical">
                <Radio value="template">
                  稳定模式（推荐）：生成速度较快，适合日常课件、普通汇报和快速成稿
                </Radio>
                <Radio value="agent">
                  ppt-master 原生实验模式：使用原生项目结构和质量检查链路，耗时较长，稳定性仍在实验中
                </Radio>
              </Space>
            </Radio.Group>
            {generationMode === "agent" && (
              <div className="mt-4 border-t border-slate-200 pt-4">
                <div className="flex items-center justify-between gap-4">
                  <div>
                    <div className="font-medium">严格质量检查</div>
                    <Text type="secondary">
                      开启：页面达到修复次数后仍不合格，则停止生成。关闭：页面仍不合格也继续生成，并尝试导出 PPTX。
                    </Text>
                  </div>
                  <Switch
                    checked={blockOnQualityFailure}
                    onChange={setBlockOnQualityFailure}
                    aria-label="严格质量检查"
                  />
                </div>
              </div>
            )}
          </Card>

          <Space wrap>
            <Button
              type="primary"
              icon={<Play size={15} />}
              loading={generating}
              onClick={handleConfirmGenerate}
            >
              {generating ? "正在生成 PPTX..." : "确认以上理解并生成 PPT"}
            </Button>
            <Button
              icon={<FileDown size={15} />}
              loading={understandingExporting}
              disabled={!understandingDraft}
              onClick={handleExportUnderstandingMarkdown}
            >
              导出理解为 MD
            </Button>
          </Space>

          {understandingExportError && (
            <Alert
              type="error"
              showIcon
              message="AI 理解导出失败"
              description={understandingExportError}
            />
          )}

          {lastUnderstandingExportPath && (
            <div className="flex flex-wrap items-center gap-2 text-sm text-slate-500">
              <span className="max-w-full truncate">最近导出：{lastUnderstandingExportPath}</span>
              <Button
                type="link"
                size="small"
                onClick={() => openPath(lastUnderstandingExportPath).catch((error) => message.error(String(error)))}
              >
                打开文件
              </Button>
              <Button
                type="link"
                size="small"
                onClick={() => revealItemInDir(lastUnderstandingExportPath).catch((error) => message.error(String(error)))}
              >
                打开所在文件夹
              </Button>
            </div>
          )}

          {(generating || generationResult || generationError) && (
            <Card title="智能生成结果">
              {generationError && (
                <Alert
                  className="mb-4"
                  type="error"
                  showIcon
                  message="生成未进入或执行失败"
                  description={generationError}
                />
              )}
              {generating && (
                <Alert
                  type="info"
                  showIcon
                  message={
                    generationMode === "agent"
                      ? "正在使用 ppt-master 原生实验模式生成，请稍候..."
                      : "正在使用稳定模式生成 PPTX，请稍候..."
                  }
                  description={
                    <div className="space-y-1">
                      {generationMode === "agent" && (
                        <>
                          <div>本次不会使用 example 示例项目。</div>
                          <div>预计项目位置：ppt-master/projects/pome_ppt_xxx</div>
                          <div>规划设计规范 design_spec.md</div>
                          <div>逐页生成 SVG</div>
                          <div>检查 SVG 质量</div>
                          <div>导出可编辑 PPTX</div>
                        </>
                      )}
                      {generationMode !== "agent" && (
                        <>
                          <div>生成 SVG 页面</div>
                          <div>导出可编辑 PPTX</div>
                        </>
                      )}
                    </div>
                  }
                />
              )}

              {generationResult && (
                <div className="space-y-4">
                  <Alert
                    type={generationResult.success ? "success" : "error"}
                    showIcon
                    message={generationResult.success ? "生成成功" : "生成失败"}
                    description={
                      <div className="space-y-1">
                        <div>原始生成 PPTX 路径 pptxPath：{generationResult.pptxPath ?? "未生成"}</div>
                        <div>
                          最终导出路径 finalPptxPath：
                          {generationResult.finalPptxPath ?? "未设置导出文件夹或尚未复制"}
                        </div>
                        <div>耗时：{generationResult.durationMs} ms</div>
                        {generationResult.error && <div>error：{generationResult.error}</div>}
                        {generationMode === "agent" && (
                          <>
                            <div>新项目路径 projectPath：{generationResult.projectPath ?? "未生成"}</div>
                            <div>生成引擎 generationEngine：{generationResult.generationEngine ?? "未知"}</div>
                            <div>生成模式 generationMode：{generationResult.generationMode}</div>
                            <div>design_spec.md 路径 designSpecPath：{generationResult.designSpecPath ?? "未生成"}</div>
                            <div>slide_plan.json 路径 slidePlanPath：{generationResult.slidePlanPath ?? "未生成"}</div>
                            <div>
                              SVG 质量检查 qualityCheckPassed：
                              {generationResult.qualityCheckPassed === null
                                ? "未运行"
                                : generationResult.qualityCheckPassed
                                  ? "通过"
                                  : "未通过"}
                            </div>
                            <div>exit code：{generationResult.exitCode ?? "未知"}</div>
                          </>
                        )}
                      </div>
                    }
                  />

                  {!generationResult.success && (
                    <Alert
                      type="warning"
                      showIcon
                      message="请检查生成配置"
                      description={
                        <ol className="m-0 pl-5">
                          <li>ppt-master 根目录是否配置正确</li>
                          <li>Python 路径是否配置正确</li>
                          <li>AI 模型是否可用</li>
                          <li>导出文件夹是否存在</li>
                        </ol>
                      }
                    />
                  )}

                  <Space wrap>
                    <Button
                      icon={<ExternalLink size={15} />}
                      disabled={!generationPptPath}
                      onClick={() => generationPptPath && openPath(generationPptPath)}
                    >
                      打开 PPT
                    </Button>
                    <Button
                      icon={<FolderOpen size={15} />}
                      disabled={!generationOutputFolder}
                      onClick={() => generationOutputFolder && openPath(generationOutputFolder)}
                    >
                      打开导出文件夹
                    </Button>
                    {generationMode === "agent" && (
                      <Button
                        icon={<FolderOpen size={15} />}
                        disabled={!generationResult.projectPath}
                        onClick={() => generationResult.projectPath && openPath(generationResult.projectPath)}
                      >
                        打开项目文件夹
                      </Button>
                    )}
                  </Space>

                  {generationOutputFolder && (
                    <Text type="secondary">当前打开导出文件夹目标：{generationOutputFolder}</Text>
                  )}
                </div>
              )}
            </Card>
          )}

          {(generating || generationResult) && (
            <>
              <Card title="生成 stdout">
                <Input.TextArea
                  value={generationResult?.stdout ?? ""}
                  readOnly
                  autoSize={{ minRows: 5, maxRows: 10 }}
                  placeholder={generating ? "正在等待生成输出..." : "生成后显示 stdout"}
                />
              </Card>
              <Card title="生成 stderr">
                <Input.TextArea
                  value={generationResult?.stderr ?? ""}
                  readOnly
                  autoSize={{ minRows: 5, maxRows: 10 }}
                  placeholder={generating ? "正在等待错误输出..." : "生成后显示 stderr"}
                />
              </Card>
            </>
          )}
        </div>
      )}
    </div>
  );

  const advancedExport = (
    <div className="space-y-4">
      <Alert
        type="warning"
        showIcon
        message="这是开发者测试入口：用于选择一个已经包含 svg_output 的 ppt-master 项目，并导出为可编辑 PPTX。普通用户不需要使用这里。开发阶段使用本地 ppt-master 引擎路径；打包阶段生成引擎将作为应用资源内置，用户无需手动下载。"
      />

      <Card>
        <Form form={advancedForm} layout="vertical">
          <Form.Item
            name="pptMasterRoot"
            label="ppt-master 根目录"
            rules={[{ required: true, message: "请选择 ppt-master 根目录" }]}
          >
            <Input
              placeholder="例如 D:\\path\\ppt-master"
              addonAfter={
                <Button
                  type="text"
                  size="small"
                  icon={<FolderOpen size={14} />}
                  onClick={() => pickDirectory("pptMasterRoot")}
                />
              }
            />
          </Form.Item>

          <Form.Item
            name="pythonPath"
            label="Python 可执行文件路径"
            rules={[{ required: true, message: "请选择 Python 可执行文件" }]}
          >
            <Input
              placeholder="例如 .\\.venv\\Scripts\\python.exe"
              addonAfter={<Button type="text" size="small" icon={<FolderOpen size={14} />} onClick={pickPython} />}
            />
          </Form.Item>

          <Form.Item
            name="projectPath"
            label="ppt-master 项目目录"
            rules={[{ required: true, message: "请选择用于调试的 ppt-master 项目目录" }]}
            extra="仅用于开发者调试。普通用户生成 PPT 不依赖这个字段。"
          >
            <Input
              placeholder="请选择一个已包含 svg_output 的 ppt-master 项目；示例项目仅用于测试"
              addonAfter={
                <Button
                  type="text"
                  size="small"
                  icon={<FolderOpen size={14} />}
                  onClick={() => pickDirectory("projectPath")}
                />
              }
            />
          </Form.Item>

          {selectedDebugProjectIsExample && (
            <Alert
              className="mb-4"
              type="warning"
              showIcon
              message="当前选择的是 ppt-master 示例项目，导出的将是示例 PPT，不是根据用户语料生成的新 PPT。"
            />
          )}

          <Space wrap>
            <Button icon={<SearchCheck size={15} />} loading={checking} onClick={handleCheck}>
              检测环境
            </Button>
            <Button type="primary" icon={<Play size={15} />} loading={exporting} onClick={handleExport}>
              导出 PPTX
            </Button>
            <Button
              icon={<ExternalLink size={15} />}
              disabled={!canOpenOutput}
              onClick={() => exportResult?.outputPath && openPath(exportResult.outputPath)}
            >
              打开调试 PPT
            </Button>
            <Button
              icon={<FolderOpen size={15} />}
              disabled={!canOpenOutput}
              onClick={() => exportResult?.outputPath && revealItemInDir(exportResult.outputPath)}
            >
              打开调试导出文件夹
            </Button>
          </Space>
        </Form>
      </Card>

      {(checkResult || exportResult) && (
        <Card title="开发者调试导出结果">
          <Alert
            className="mb-4"
            type="info"
            showIcon
            message="这里导出的可能是示例项目，不代表智能生成结果。普通用户请使用“智能生成”Tab。"
          />
          <Alert
            type={status}
            showIcon
            message={
              exportResult
                ? exportResult.success
                  ? "调试导出成功"
                  : "调试导出失败"
                : checkResult?.ok
                  ? "环境检测通过"
                  : "环境检测未通过"
            }
            description={
              <div className="space-y-1">
                {checkResult && (
                  <>
                    <div>脚本路径：{checkResult.scriptPath}</div>
                    <div>Python：{checkResult.pythonVersion ?? "未检测到"}</div>
                    {checkResult.errors.map((err) => (
                      <div key={err}>{err}</div>
                    ))}
                  </>
                )}
                {exportResult && (
                  <>
                    <div>exit code：{exportResult.exitCode ?? "未知"}</div>
                    <div>耗时：{exportResult.durationMs} ms</div>
                    <div>调试生成的 pptx 路径：{exportResult.outputPath ?? "未生成"}</div>
                    {exportResult.error && <div>{exportResult.error}</div>}
                  </>
                )}
              </div>
            }
          />
        </Card>
      )}

      <Card title="调试 stdout">
        <Input.TextArea
          value={exportResult?.stdout ?? ""}
          readOnly
          autoSize={{ minRows: 6, maxRows: 12 }}
          placeholder="导出后显示 stdout"
        />
      </Card>

      <Card title="调试 stderr">
        <Input.TextArea
          value={exportResult?.stderr ?? ""}
          readOnly
          autoSize={{ minRows: 6, maxRows: 12 }}
          placeholder="导出后显示 stderr"
        />
      </Card>
    </div>
  );

  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto max-w-5xl p-6">
        <div className="mb-4">
          <Title level={3} style={{ marginBottom: 4 }}>
            AI PPT 生成
          </Title>
          <Text type="secondary">
            输入资料与汇报要求，AI 会先理解你的表达目标，再生成可编辑 PPTX。
          </Text>
        </div>

        <Card className="mb-4">
          <Steps
            current={currentStep}
            items={[
              { title: "输入语料" },
              { title: "确认需求" },
              { title: "生成 PPTX" },
            ]}
          />
        </Card>

        <Tabs
          activeKey={activeMode}
          onChange={(key) => setActiveMode(key as "smart" | "advanced")}
          items={[
            { key: "smart", label: "智能生成", children: smartGenerate },
            { key: "advanced", label: "开发者调试", children: advancedExport },
          ]}
        />
      </div>
    </div>
  );
}


