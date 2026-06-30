import { useEffect, useMemo, useState } from "react";
import Markdown from "react-markdown";
import {
  Alert,
  Button,
  Card,
  Form,
  Input,
  InputNumber,
  Radio,
  Select,
  Space,
  Steps,
  Tabs,
  Typography,
  message,
} from "antd";
import { Copy, ExternalLink, FolderOpen, Play, RotateCcw, SearchCheck, Wand2 } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { aiModelApi, aiWriteApi, configApi, pptMasterApi } from "@/lib/api";
import type {
  AiModel,
  PptMasterCheckResult,
  PptMasterExportResult,
  PptMasterGenerateResult,
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

interface SmartFormValues {
  topic: string;
  sourceMaterial: string;
  audience: string;
  customAudience?: string;
  pageCount: string;
  customPageCount?: number;
  style: string;
  customStyle?: string;
  extraRequirements?: string;
}

const audienceOptions = ["老师/评委", "企业客户", "课堂展示", "项目组内部", "自定义"];
const pageCountOptions = ["4 页", "6 页", "8 页", "10 页", "自定义"];
const styleOptions = ["简约商务", "科技蓝", "学术汇报", "竞赛路演", "图文并茂", "自定义"];
const EXAMPLE_PROJECT_MARKER = "examples/ppt169_lin_huiyin_architect_revised";

async function getConfigOrEmpty(key: string): Promise<string> {
  try {
    return await configApi.get(key);
  } catch {
    return "";
  }
}

interface FinalSmartValues {
  topic: string;
  sourceMaterial: string;
  audience: string;
  pageCount: string;
  style: string;
  extraRequirements: string;
}

interface AiUnderstandingSections {
  summary: string;
  tradeoffs: string;
  storyline: string;
  pageStructure: string;
  visualAdvice: string;
  questions: string;
  confirmPrompt: string;
  raw: string;
}

function buildAiUnderstandingPrompt(values: FinalSmartValues): string {
  return `你是一名 PPT 策划专家和比赛汇报教练。用户会提供 PPT 主题、原始语料、汇报对象、页数、风格和额外要求。你的任务不是复述资料，而是判断这份 PPT 应该如何组织，生成一份“用户可确认的 PPT 制作理解结果”。

请输出中文，结构清晰，尽量精简。不要大段复制原始语料。重点回答：

1. 这份 PPT 的核心目标是什么？
2. 应该讲给谁听？听众最关心什么？
3. 这份材料里最值得突出的 3-5 个重点是什么？
4. 哪些内容应该弱化或合并？
5. 推荐的叙事主线是什么？
6. 建议页面结构，每页一句话说明。
7. 推荐视觉风格和版式倾向。
8. 如果语料不足，指出还缺哪些信息。
9. 最后生成一段“确认用精简 Prompt”，供用户确认或修改。

用户输入如下：
【主题】
${values.topic}

【汇报对象】
${values.audience}

【页数】
${values.pageCount}

【风格】
${values.style}

【额外要求】
${values.extraRequirements}

【原始语料】
${values.sourceMaterial}

输出格式必须是：

【AI理解摘要】
用 2-4 句话概括你认为这份 PPT 应该做成什么样。

【重点取舍】
* 应突出：
  1.
  2.
  3.
* 应弱化/合并：
  1.
  2.

【叙事主线】
一句话说明推荐的讲述逻辑。

【建议页面结构】
1. xxx：一句话说明
2. xxx：一句话说明
...

【视觉与表达建议】
用 2-4 条短句说明风格、版式、图文比例。

【仍需确认的问题】
如果没有明显问题，写“暂无，当前信息足够生成初版 PPT。”
如果有问题，列出 1-3 个问题。

【确认用精简 Prompt】
用 150-300 字，写成用户能看懂、能确认的最终生成要求。
这段不要包含长篇原始语料。`;
}

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

function parseAiUnderstanding(raw: string): AiUnderstandingSections {
  const confirmPrompt = cleanAiMarkdown(extractSection(raw, "确认用精简 Prompt") || raw.trim());
  return {
    summary: cleanAiMarkdown(extractSection(raw, "AI理解摘要")),
    tradeoffs: cleanAiMarkdown(extractSection(raw, "重点取舍")),
    storyline: cleanAiMarkdown(extractSection(raw, "叙事主线")),
    pageStructure: cleanAiMarkdown(extractSection(raw, "建议页面结构")),
    visualAdvice: cleanAiMarkdown(extractSection(raw, "视觉与表达建议")),
    questions: cleanAiMarkdown(extractSection(raw, "仍需确认的问题")),
    confirmPrompt,
    raw: cleanAiMarkdown(raw),
  };
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
  const [advancedForm] = Form.useForm<AdvancedFormValues>();
  const [smartForm] = Form.useForm<SmartFormValues>();
  const [activeMode, setActiveMode] = useState("smart");
  const [checking, setChecking] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [understanding, setUnderstanding] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [loadingModels, setLoadingModels] = useState(false);
  const [aiModels, setAiModels] = useState<AiModel[]>([]);
  const [selectedModelId, setSelectedModelId] = useState<number | null>(null);
  const [generationMode, setGenerationMode] = useState<"agent" | "template">("agent");
  const [outputDir, setOutputDir] = useState("");
  const [checkResult, setCheckResult] = useState<PptMasterCheckResult | null>(null);
  const [exportResult, setExportResult] = useState<PptMasterExportResult | null>(null);
  const [generationResult, setGenerationResult] = useState<PptMasterGenerateResult | null>(null);
  const [confirmedPrompt, setConfirmedPrompt] = useState("");
  const [aiUnderstanding, setAiUnderstanding] = useState<AiUnderstandingSections | null>(null);
  const selectedAudience = Form.useWatch("audience", smartForm);
  const selectedPageCount = Form.useWatch("pageCount", smartForm);
  const selectedStyle = Form.useWatch("style", smartForm);
  const watchedPptMasterRoot = Form.useWatch("pptMasterRoot", advancedForm);
  const watchedPythonPath = Form.useWatch("pythonPath", advancedForm);
  const watchedProjectPath = Form.useWatch("projectPath", advancedForm);

  useEffect(() => {
    void (async () => {
      const [pptMasterRoot, pythonPath, projectPath, savedOutputDir] = await Promise.all([
        getConfigOrEmpty(CONFIG_KEYS.pptMasterRoot),
        getConfigOrEmpty(CONFIG_KEYS.pythonPath),
        getConfigOrEmpty(CONFIG_KEYS.defaultProjectPath),
        getConfigOrEmpty(CONFIG_KEYS.outputDir),
      ]);
      advancedForm.setFieldsValue({ pptMasterRoot, pythonPath, projectPath });
      setOutputDir(savedOutputDir);
    })();
  }, [advancedForm]);

  useEffect(() => {
    void (async () => {
      setLoadingModels(true);
      try {
        const [models, savedModelId] = await Promise.all([
          aiModelApi.list(),
          getConfigOrEmpty(CONFIG_KEYS.aiModelId),
        ]);
        setAiModels(models);
        setSelectedModelId(chooseInitialModelId(models, savedModelId));
      } catch (e) {
        message.error(`加载 AI 模型失败: ${e}`);
      } finally {
        setLoadingModels(false);
      }
    })();
  }, []);

  const canOpenOutput = !!exportResult?.outputPath;
  const selectedModel = useMemo(
    () => aiModels.find((model) => model.id === selectedModelId) ?? null,
    [aiModels, selectedModelId],
  );
  const engineConfigured = !!watchedPptMasterRoot?.trim() && !!watchedPythonPath?.trim();
  const selectedDebugProjectIsExample = isLinHuiyinExampleProject(watchedProjectPath);
  const currentStep = useMemo(() => {
    if (activeMode === "smart" && (generating || generationResult)) {
      return 2;
    }
    if (activeMode === "smart" && confirmedPrompt) {
      return 1;
    }
    return 0;
  }, [activeMode, confirmedPrompt, generating, generationResult]);

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

  function resolveSmartValues(values: SmartFormValues): FinalSmartValues | null {
    const topic = values.topic?.trim();
    const sourceMaterial = values.sourceMaterial?.trim();
    const audience =
      values.audience === "自定义" ? values.customAudience?.trim() : values.audience;
    const pageCount =
      values.pageCount === "自定义"
        ? values.customPageCount
          ? `${values.customPageCount} 页`
          : ""
        : values.pageCount;
    const style = values.style === "自定义" ? values.customStyle?.trim() : values.style;

    if (!topic || !sourceMaterial || !audience || !pageCount || !style) {
      message.warning("请完整填写主题、语料、汇报对象、页数和风格");
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

  async function handleUnderstandNeeds() {
    if (!selectedModelId) {
      message.warning("尚未配置 AI 模型，请先到设置页添加模型。");
      return;
    }

    const values = await smartForm.validateFields();
    const finalValues = resolveSmartValues(values);
    if (!finalValues) {
      return;
    }

    setUnderstanding(true);
    setAiUnderstanding(null);
    setConfirmedPrompt("");
    try {
      const raw = await aiWriteApi.understandPpt({
        prompt: buildAiUnderstandingPrompt(finalValues),
        modelId: selectedModelId,
      });
      const parsed = parseAiUnderstanding(raw);
      setAiUnderstanding(parsed);
      setConfirmedPrompt(parsed.confirmPrompt);
      message.success("AI 已完成需求理解");
    } catch (e) {
      message.error(String(e));
    } finally {
      setUnderstanding(false);
    }
  }

  function handleClearSmartForm() {
    smartForm.resetFields();
    setConfirmedPrompt("");
    setAiUnderstanding(null);
  }

  async function handleCopyPrompt() {
    if (!confirmedPrompt.trim()) {
      message.warning("请先生成或填写 Prompt");
      return;
    }
    await navigator.clipboard.writeText(confirmedPrompt);
    message.success("Prompt 已复制");
  }

  async function handleConfirmGenerate() {
    const prompt = confirmedPrompt.trim();
    if (!prompt) {
      message.warning("请先确认或填写精简 Prompt");
      return;
    }

    const advancedValues = advancedForm.getFieldsValue();
    const [savedPptMasterRoot, savedPythonPath] = await Promise.all([
      getConfigOrEmpty(CONFIG_KEYS.pptMasterRoot),
      getConfigOrEmpty(CONFIG_KEYS.pythonPath),
    ]);
    const pptMasterRoot = savedPptMasterRoot.trim() || advancedValues.pptMasterRoot?.trim();
    const pythonPath = savedPythonPath.trim() || advancedValues.pythonPath?.trim();
    if (!pptMasterRoot || !pythonPath) {
      message.error("生成引擎尚未配置。开发阶段请先到“开发者调试”填写 ppt-master 根目录和 Python 路径；打包版本将内置生成引擎。");
      return;
    }

    const smartValues = smartForm.getFieldsValue();
    setGenerating(true);
    setGenerationResult(null);
    setExportResult(null);
    setCheckResult(null);
    try {
      const result = await pptMasterApi.generateFromPrompt({
        pptMasterRoot,
        pythonPath,
        prompt,
        modelId: selectedModelId,
        title: smartValues.topic?.trim() || undefined,
        slideCount: getCurrentSlideCount(smartValues),
        style: getCurrentStyle(smartValues),
        outputDir: outputDir.trim() || null,
        generationMode,
      });
      setGenerationResult(result);
      if (result.success) {
        message.success("PPTX 生成完成");
      } else {
        message.error(result.error ?? "PPTX 生成失败");
      }
    } catch (e) {
      message.error(String(e));
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
        error: String(e),
      });
    } finally {
      setGenerating(false);
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

  function renderAnalysisCard(title: string, content?: string) {
    if (!content?.trim()) {
      return null;
    }
    return (
      <Card title={title}>
        <div className="ai-markdown">
          <Markdown>{cleanAiMarkdown(content)}</Markdown>
        </div>
      </Card>
    );
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
          initialValues={{
            audience: "老师/评委",
            pageCount: "6 页",
            style: "科技蓝",
          }}
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
            <Input placeholder="例如：Pomegranate 与 PPT Master 融合方案" />
          </Form.Item>

          <Form.Item
            name="sourceMaterial"
            label="原始语料"
            rules={[{ required: true, message: "请粘贴原始语料" }]}
          >
            <Input.TextArea
              autoSize={{ minRows: 8, maxRows: 16 }}
              placeholder="粘贴笔记、文档摘要、项目说明、实验内容等"
            />
          </Form.Item>

          <div className="grid gap-4 md:grid-cols-3">
            <Form.Item name="audience" label="汇报对象" rules={[{ required: true }]}>
              <Select options={audienceOptions.map((value) => ({ value, label: value }))} />
            </Form.Item>

            <Form.Item name="pageCount" label="页数" rules={[{ required: true }]}>
              <Select options={pageCountOptions.map((value) => ({ value, label: value }))} />
            </Form.Item>

            <Form.Item name="style" label="风格" rules={[{ required: true }]}>
              <Select options={styleOptions.map((value) => ({ value, label: value }))} />
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
                  <Input placeholder="例如：北理工官网风、蓝白答辩风、黑金科技风" />
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
              清空
            </Button>
          </Space>
        </Form>
      </Card>

      {aiUnderstanding && (
        <div className="space-y-4">
          {renderAnalysisCard("AI 理解摘要", aiUnderstanding.summary)}
          {renderAnalysisCard("重点取舍", aiUnderstanding.tradeoffs)}
          {renderAnalysisCard("叙事主线", aiUnderstanding.storyline)}
          {renderAnalysisCard("建议页面结构", aiUnderstanding.pageStructure)}
          {renderAnalysisCard("视觉与表达建议", aiUnderstanding.visualAdvice)}
          {renderAnalysisCard("仍需确认的问题", aiUnderstanding.questions)}
          {!aiUnderstanding.summary && (
            <Card title="AI 完整输出">
              <div className="ai-markdown">
                <Markdown>{cleanAiMarkdown(aiUnderstanding.raw)}</Markdown>
              </div>
            </Card>
          )}

          <Card title="确认用精简 Prompt">
            <Input.TextArea
              value={confirmedPrompt}
              onChange={(e) => setConfirmedPrompt(e.target.value)}
              autoSize={{ minRows: 8, maxRows: 16 }}
            />
            <div className="mt-4">
              <Text strong>生成模式</Text>
              <Radio.Group
                className="mt-2 block"
                value={generationMode}
                onChange={(e) => setGenerationMode(e.target.value)}
              >
                <Radio value="agent">
                  精美模式（推荐）：调用 ppt-master Agent 工作流，效果更好，耗时更长
                </Radio>
                <Radio value="template">
                  快速模式：模板生成，速度快但效果一般
                </Radio>
              </Radio.Group>
            </div>
            <Space wrap className="mt-4">
              <Button
                type="primary"
                icon={<Play size={15} />}
                loading={generating}
                onClick={handleConfirmGenerate}
              >
                {generating ? "正在生成 PPTX..." : "确认并生成 PPT"}
              </Button>
              <Button icon={<Copy size={15} />} onClick={handleCopyPrompt}>
                复制 Prompt
              </Button>
            </Space>
          </Card>

          {(generating || generationResult) && (
            <Card title="智能生成结果">
              {generating && (
                <Alert
                  type="info"
                  showIcon
                  message={
                    generationMode === "agent"
                      ? "正在使用 ppt-master 精美工作流生成，请稍候..."
                      : "正在创建新的 PPT 项目，请稍候..."
                  }
                  description={
                    <div className="space-y-1">
                      <div>本次不会使用 example 示例项目。</div>
                      <div>预计项目位置：ppt-master/projects/pome_ppt_xxx</div>
                      {generationMode === "agent" && (
                        <>
                          <div>规划设计规范 design_spec.md</div>
                          <div>逐页生成 SVG</div>
                          <div>检查 SVG 质量</div>
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
                        <div>新项目路径 projectPath：{generationResult.projectPath ?? "未生成"}</div>
                        <div>生成模式 generationMode：{generationResult.generationMode}</div>
                        <div>design_spec.md 路径 designSpecPath：{generationResult.designSpecPath ?? "未生成"}</div>
                        <div>slide_plan.json 路径 slidePlanPath：{generationResult.slidePlanPath ?? "未生成"}</div>
                        <div>原始生成 PPTX 路径 pptxPath：{generationResult.pptxPath ?? "未生成"}</div>
                        <div>
                          最终导出路径 finalPptxPath：
                          {generationResult.finalPptxPath ?? "未设置导出文件夹或尚未复制"}
                        </div>
                        <div>
                          SVG 质量检查 qualityCheckPassed：
                          {generationResult.qualityCheckPassed === null
                            ? "未运行"
                            : generationResult.qualityCheckPassed
                              ? "通过"
                              : "未通过"}
                        </div>
                        <div>exit code：{generationResult.exitCode ?? "未知"}</div>
                        <div>耗时：{generationResult.durationMs} ms</div>
                        {generationResult.error && <div>error：{generationResult.error}</div>}
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
                    <Button
                      icon={<FolderOpen size={15} />}
                      disabled={!generationResult.projectPath}
                      onClick={() => generationResult.projectPath && openPath(generationResult.projectPath)}
                    >
                      打开项目文件夹
                    </Button>
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
          onChange={setActiveMode}
          items={[
            { key: "smart", label: "智能生成", children: smartGenerate },
            { key: "advanced", label: "开发者调试", children: advancedExport },
          ]}
        />
      </div>
    </div>
  );
}


