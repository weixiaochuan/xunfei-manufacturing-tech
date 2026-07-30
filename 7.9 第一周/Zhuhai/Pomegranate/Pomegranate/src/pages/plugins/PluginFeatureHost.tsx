import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Form,
  Input,
  InputNumber,
  List,
  Select,
  Space,
  Spin,
  Switch,
  Typography,
  message,
} from "antd";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ArrowLeft, FileDown, Play } from "lucide-react";
import ReactMarkdown from "react-markdown";
import { useNavigate, useParams } from "react-router-dom";

import { externalAgentApi, pluginApi } from "@/lib/api";
import {
  runPluginPipelineAfterModel,
  runPluginPipelineBeforeModel,
} from "@/services/pluginPipeline";
import type {
  ExternalAgentConfig,
  PluginFeatureContributionV3,
  PluginInfo,
  PluginScene,
  WorkflowGeneratedFile,
} from "@/types";

const { Paragraph, Text, Title } = Typography;

type FieldKind =
  | "text"
  | "string"
  | "textarea"
  | "multiline"
  | "integer"
  | "number"
  | "select"
  | "switch"
  | "boolean"
  | "json"
  | "file"
  | "files";

interface DeclarativeField {
  id?: string;
  key?: string;
  label: string;
  type: FieldKind;
  required?: boolean;
  placeholder?: string;
  description?: string;
  defaultValue?: unknown;
  rows?: number;
  sensitive?: boolean;
  options?: Array<{ label: string; value: string | number | boolean }>;
}

interface DeclarativeFeatureSchema {
  title?: string;
  description?: string;
  fields?: DeclarativeField[];
  submitLabel?: string;
  outputTitle?: string;
  outputTemplate?: string;
  output?: {
    kind?: "text" | "markdown" | "json" | "docx-base64" | "file-base64";
  };
}

function parseSchema(content: string): DeclarativeFeatureSchema {
  const parsed: unknown = JSON.parse(content);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("声明式 UI Schema 必须是 JSON 对象");
  }
  const schema = parsed as DeclarativeFeatureSchema;
  for (const field of schema.fields ?? []) {
    const key = field.key ?? field.id;
    if (!key || !/^[A-Za-z_][A-Za-z0-9_.-]*$/.test(key)) {
      throw new Error("uiSchema 包含非法或缺失的字段 key");
    }
    if (field.sensitive || /api.?key|api.?secret|token|password|authorization/i.test(key)) {
      throw new Error("插件表单不得收集凭据明文，请在 AI 资源中心绑定凭据");
    }
  }
  return schema;
}

function fieldKey(field: DeclarativeField) {
  return field.key ?? field.id ?? "";
}

function renderTemplate(template: string, values: Record<string, unknown>) {
  return template.replace(/\{\{([a-zA-Z0-9_.-]+)\}\}/g, (_match, key: string) => {
    const value = values[key];
    return value === undefined || value === null ? "" : String(value);
  });
}

function usableAgent(agent: ExternalAgentConfig) {
  return agent.enabled
    && !agent.unavailableReason
    && (agent.mockMode || Boolean(agent.credentialId))
    && (agent.protocolType === "xingchen_workflow_v1" || agent.mockMode);
}

function normalizeParameters(
  fields: DeclarativeField[],
  values: Record<string, unknown>,
) {
  const parameters: Record<string, unknown> = {};
  const filePaths: Record<string, string[]> = {};
  for (const field of fields) {
    const key = fieldKey(field);
    const value = values[key];
    if (value === undefined || value === null || value === "") continue;
    if (field.type === "file") {
      filePaths[key] = [String(value)];
      continue;
    }
    if (field.type === "files") {
      filePaths[key] = Array.isArray(value) ? value.map(String) : [String(value)];
      continue;
    }
    if (field.type === "json") {
      try {
        parameters[key] = typeof value === "string" ? JSON.parse(value) : value;
      } catch {
        throw new Error(`${field.label} 不是合法 JSON`);
      }
      continue;
    }
    parameters[key] = value;
  }
  return { parameters, filePaths };
}

function pipelineFeatureValues(
  beforeInput: unknown,
  originalValues: Record<string, unknown>,
  fields: DeclarativeField[],
) {
  if (beforeInput && typeof beforeInput === "object" && !Array.isArray(beforeInput)) {
    return beforeInput as Record<string, unknown>;
  }
  if (typeof beforeInput !== "string") return originalValues;
  const target = fields.find((field) => fieldKey(field) === "AGENT_USER_INPUT")
    ?? fields.find((field) =>
      ["text", "string", "textarea", "multiline"].includes(field.type),
    );
  if (!target) return originalValues;
  return { ...originalValues, [fieldKey(target)]: beforeInput };
}

export default function PluginFeatureHost() {
  const { pluginId = "", featureId = "" } = useParams();
  const navigate = useNavigate();
  const [form] = Form.useForm<Record<string, unknown>>();
  const [feature, setFeature] = useState<PluginFeatureContributionV3 | null>(null);
  const [featureScene, setFeatureScene] = useState<PluginScene>("global");
  const [plugin, setPlugin] = useState<PluginInfo | null>(null);
  const [schema, setSchema] = useState<DeclarativeFeatureSchema | null>(null);
  const [agents, setAgents] = useState<ExternalAgentConfig[]>([]);
  const [externalAgentId, setExternalAgentId] = useState<string>();
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [output, setOutput] = useState("");
  const [outputFiles, setOutputFiles] = useState<WorkflowGeneratedFile[]>([]);
  const [mockResult, setMockResult] = useState(false);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      setLoading(true);
      setError(null);
      try {
        const scenes: PluginScene[] = ["global", "learning", "research", "teaching"];
        const [results, installedPlugins, configuredAgents] = await Promise.all([
          Promise.all(scenes.map(async (scene) => ({
            scene,
            result: await pluginApi.resolveEnabledContributions({
              scene,
              feature: featureId,
              requestId: `feature-host-${Date.now()}-${scene}`,
              selectedResources: [],
              metadata: {},
              sessionOverrides: {},
            }),
          }))),
          pluginApi.list(),
          externalAgentApi.list(),
        ]);
        const matched = results
          .flatMap(({ scene, result }) => result.features.map((item) => ({ scene, item })))
          .find(({ item }) => item.pluginId === pluginId && item.id === featureId);
        if (!matched) throw new Error("功能未启用、当前场景不支持，或贡献不存在");
        if (!matched.item.uiSchema) throw new Error("该功能未声明 uiSchema");
        const installed = installedPlugins.find((item) => item.id === pluginId);
        if (!installed) throw new Error("插件未安装");
        const content = await pluginApi.readFeatureUiSchema(pluginId, featureId);
        if (cancelled) return;
        const availableAgents = configuredAgents.filter(usableAgent);
        setFeature(matched.item);
        setFeatureScene(matched.scene);
        setPlugin(installed);
        setSchema(parseSchema(content));
        setAgents(availableAgents);
        setExternalAgentId(availableAgents[0]?.id);
      } catch (loadError) {
        if (!cancelled) setError(String(loadError));
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    void load();
    return () => { cancelled = true; };
  }, [featureId, pluginId]);

  const isXingchen = plugin?.runtimeKind === "xingchen-workflow"
    || plugin?.runtimeKind === "xingchen-agent";
  const fields = useMemo<DeclarativeField[]>(() => {
    const configured = schema?.fields ?? [];
    if (configured.length > 0 || !isXingchen) return configured;
    return [{
      key: "AGENT_USER_INPUT",
      label: "用户输入",
      type: "multiline",
      required: true,
      placeholder: "请输入要发送给 Workflow 的内容",
      description: "旧插件未声明输入字段时使用的兼容字段。",
    }];
  }, [isXingchen, schema]);
  const initialValues = useMemo(() => Object.fromEntries(
    fields
      .filter((field) => field.defaultValue !== undefined)
      .map((field) => [fieldKey(field), field.defaultValue]),
  ), [fields]);

  async function pickFile(field: DeclarativeField) {
    const selected = await openDialog({
      directory: false,
      multiple: field.type === "files",
      title: "选择文件",
    });
    if (!selected) return;
    form.setFieldValue(fieldKey(field), selected);
  }

  async function submit(values: Record<string, unknown>) {
    if (!schema) return;
    setRunning(true);
    setError(null);
    setOutput("");
    setOutputFiles([]);
    try {
      const before = await runPluginPipelineBeforeModel({
        scene: featureScene,
        feature: featureId,
        input: values,
        workspaceId: `plugin:${pluginId}`,
        selectedResources: [],
        metadata: {
          pluginId,
          runtimeKind: plugin?.runtimeKind,
        },
      });
      if (before.warnings.length > 0) {
        message.warning(`有 ${before.warnings.length} 个插件增强步骤未完全执行，已继续原功能`);
      }
      if (!isXingchen) {
        const template = schema.outputTemplate
          ?? "## 提交结果\n\n\`\`\`json\n{{json}}\n\`\`\`";
        const rawOutput = renderTemplate(template, {
          ...values,
          json: JSON.stringify(values, null, 2),
        });
        const after = await runPluginPipelineAfterModel(before, rawOutput);
        setOutput(after.output);
        setMockResult(true);
        return;
      }
      if (!externalAgentId) {
        throw new Error("请先在 AI 资源中心配置并启用一个讯飞 Workflow 智能体");
      }
      const effectiveValues = pipelineFeatureValues(before.input, values, fields);
      const { parameters, filePaths } = normalizeParameters(fields, effectiveValues);
      const result = await pluginApi.invokeXingchenFeature({
        pluginId,
        featureId,
        externalAgentId,
        parameters,
        filePaths,
        pluginSystemContext: before.prompt || null,
        pluginContributionIds: before.executedContributionIds,
      });
      if (!result.ok) throw new Error(result.content || "Workflow 调用失败");
      const after = await runPluginPipelineAfterModel(before, result.content);
      setOutput(after.output);
      setOutputFiles(result.outputFiles ?? []);
      setMockResult(result.mock);
      message.success(result.mock ? "Mock 调用完成" : "Workflow 调用完成");
    } catch (submitError) {
      const text = String(submitError);
      setError(text);
      message.error(text);
    } finally {
      setRunning(false);
    }
  }

  function renderField(field: DeclarativeField) {
    if (field.type === "textarea" || field.type === "multiline") {
      return <Input.TextArea rows={field.rows ?? 6} placeholder={field.placeholder} />;
    }
    if (field.type === "number" || field.type === "integer") {
      return (
        <InputNumber
          precision={field.type === "integer" ? 0 : undefined}
          style={{ width: "100%" }}
          placeholder={field.placeholder}
        />
      );
    }
    if (field.type === "select") {
      return <Select options={field.options ?? []} placeholder={field.placeholder} />;
    }
    if (field.type === "switch" || field.type === "boolean") return <Switch />;
    if (field.type === "file" || field.type === "files") {
      return (
        <Space.Compact style={{ width: "100%" }}>
          <Input readOnly placeholder={field.placeholder ?? "请选择文件"} />
          <Button onClick={() => pickFile(field)}>选择</Button>
        </Space.Compact>
      );
    }
    if (field.type === "json") {
      return <Input.TextArea rows={6} placeholder={field.placeholder ?? "{ }"} />;
    }
    return <Input placeholder={field.placeholder} />;
  }

  if (loading) return <div className="flex min-h-[280px] items-center justify-center"><Spin /></div>;
  if (error && (!schema || !feature)) {
    return <Alert type="error" showIcon message="无法加载插件功能" description={error} />;
  }
  if (!schema || !feature || !plugin) return null;

  return (
    <div className="mx-auto max-w-4xl pb-8">
      <Button type="text" icon={<ArrowLeft size={16} />} onClick={() => navigate("/marketplace?section=plugins")}>
        返回插件管理
      </Button>
      <Card className="mt-3">
        <Title level={3}>{schema.title ?? feature.title}</Title>
        <Paragraph type="secondary">{schema.description ?? feature.description}</Paragraph>
        {isXingchen && (
          <>
            <Alert
              className="mb-4"
              type="warning"
              showIcon
              message="此功能会把你填写的内容发送到讯飞星辰"
              description="调用由 Rust 后端使用 AI 资源中心中的安全凭据完成，可能消耗你自己的讯飞额度；插件无法读取密钥明文。"
            />
            <Form.Item label="调用智能体" required>
              <Select
                value={externalAgentId}
                onChange={setExternalAgentId}
                placeholder="选择 AI 资源中心中已配置的 Workflow"
                options={agents.map((agent) => ({
                  value: agent.id,
                  label: `${agent.name}${agent.mockMode ? "（Mock 演示）" : "（真实 Provider）"}`,
                }))}
              />
            </Form.Item>
            {agents.length === 0 && (
              <Alert
                className="mb-4"
                type="info"
                showIcon
                message="暂无可用的讯飞 Workflow"
                description={(
                  <Button type="link" onClick={() => navigate("/ai-resources")}>
                    前往 AI 资源中心配置
                  </Button>
                )}
              />
            )}
          </>
        )}
        {error && <Alert className="mb-4" type="error" showIcon message="运行失败" description={error} />}
        <Form form={form} layout="vertical" initialValues={initialValues} onFinish={submit}>
          {fields.map((field) => (
            <Form.Item
              key={fieldKey(field)}
              name={fieldKey(field)}
              label={field.label}
              extra={field.description}
              valuePropName={field.type === "switch" || field.type === "boolean" ? "checked" : "value"}
              rules={field.required ? [{ required: true, message: `请填写${field.label}` }] : undefined}
            >
              {renderField(field)}
            </Form.Item>
          ))}
          <Button
            type="primary"
            htmlType="submit"
            icon={<Play size={16} />}
            loading={running}
            disabled={isXingchen && !externalAgentId}
          >
            {schema.submitLabel ?? "运行"}
          </Button>
        </Form>
      </Card>
      {(output || outputFiles.length > 0) && (
        <Card className="mt-4" title={schema.outputTitle ?? "运行结果"}>
          <Text type={mockResult ? "warning" : "secondary"}>
            {mockResult
              ? "Mock 演示结果，不代表真实讯飞调用成功。"
              : "结果来自你选择的外部智能体。"}
          </Text>
          {output && (
            schema.output?.kind === "json"
              ? <pre className="mt-3 max-h-96 overflow-auto rounded bg-gray-50 p-3">{output}</pre>
              : <div className="ai-markdown mt-3"><ReactMarkdown>{output}</ReactMarkdown></div>
          )}
          {outputFiles.length > 0 && (
            <List
              className="mt-3"
              dataSource={outputFiles}
              renderItem={(file) => (
                <List.Item>
                  <Space>
                    <FileDown size={16} />
                    <Text copyable={{ text: file.path }}>{file.fileName}</Text>
                    <Text type="secondary">{Math.ceil(file.size / 1024)} KB</Text>
                  </Space>
                </List.Item>
              )}
            />
          )}
        </Card>
      )}
    </div>
  );
}
