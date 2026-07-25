import { useEffect, useMemo, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import Markdown from "react-markdown";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Divider,
  Form,
  Input,
  InputNumber,
  List,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import { Bot, KeyRound, MessageSquare, RadioTower, ShieldCheck } from "lucide-react";
import { credentialApi, externalAgentApi, runtimeApi } from "@/lib/api";
import { PlanningWithFilesPanel, stripPlanningUpdateBlock } from "@/components/ai/PlanningWithFilesPanel";
import {
  DEFAULT_WORKFLOW_INPUT_KEY,
  WORKFLOW_FIELD_TYPE_OPTIONS,
  buildWorkflowRequestMapping,
  buildWorkflowSubmission,
  importWorkflowFieldsFromText,
  normalizeWorkflowInputFields,
  workflowFieldsFromMapping,
  workflowInitialValues,
  workflowPreview,
} from "@/lib/workflowSchema";
import type {
  AgentMessageInfo,
  AgentStreamEvent,
  AgentUsageEvent,
  AgentWorkflowInvokeResult,
  BindableXingchenProduct,
  CredentialInfo,
  ExternalAgentConfig,
  RuntimeDataDirectory,
  WorkflowInputField,
} from "@/types";

const { Title, Paragraph, Text } = Typography;
const XINGCHEN_WORKFLOW_V1_ENDPOINT = "https://xingchen-api.xf-yun.com/workflow/v1/chat/completions";

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timer: number | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = window.setTimeout(() => reject(new Error(message)), timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer !== undefined) window.clearTimeout(timer);
  });
}

function normalizeWorkflowException(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  const hasChinese = /[\u4e00-\u9fff]/.test(raw);
  const looksMojibake = /[ÃÂ�]|[\u0080-\u00ff]{2,}|â[\u0080-\u00ff]?|ï¼|ã€/.test(raw);
  if (looksMojibake && !hasChinese) {
    return "Workflow 调用失败：后端返回的错误信息存在编码异常。请先检查 Flow ID、凭据，以及开始节点输入字段是否与星辰工作流 schema 一致。";
  }
  return raw || "Workflow 调用失败：未知错误";
}

export default function AiResourcesPage() {
  const [credentials, setCredentials] = useState<CredentialInfo[]>([]);
  const [agents, setAgents] = useState<ExternalAgentConfig[]>([]);
  const [bindableProducts, setBindableProducts] = useState<BindableXingchenProduct[]>([]);
  const [runtimeDir, setRuntimeDir] = useState<RuntimeDataDirectory | null>(null);
  const [runtimeDirLoading, setRuntimeDirLoading] = useState(true);
  const [runtimeDirError, setRuntimeDirError] = useState<string | null>(null);
  const [usage, setUsage] = useState<AgentUsageEvent[]>([]);
  const [sessions, setSessions] = useState<any[]>([]);
  const [messages, setMessages] = useState<AgentMessageInfo[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState<string>();
  const [selectedSessionId, setSelectedSessionId] = useState<string>();
  const [credentialOpen, setCredentialOpen] = useState(false);
  const [agentOpen, setAgentOpen] = useState(false);
  const [editingAgentId, setEditingAgentId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const [streamingText, setStreamingText] = useState("");
  const [streamProgress, setStreamProgress] = useState<number | null>(null);
  const [input, setInput] = useState("");
  const [mockScenario, setMockScenario] = useState<string>("normal");
  const requestIdRef = useRef<string | null>(null);
  const [credentialForm] = Form.useForm();
  const [agentForm] = Form.useForm();
  const [workflowForm] = Form.useForm();
  const agentProtocolType = Form.useWatch("protocolType", agentForm) ?? "configurable";
  const isWorkflowV1 = agentProtocolType === "xingchen_workflow_v1";
  const [workflowSubmitting, setWorkflowSubmitting] = useState(false);
  const [workflowResult, setWorkflowResult] = useState<AgentWorkflowInvokeResult | null>(null);
  const [workflowPreviewJson, setWorkflowPreviewJson] = useState<string>("");
  const [workflowFileNames, setWorkflowFileNames] = useState<Record<string, string[]>>({});
  const [workflowSelectedFilePaths, setWorkflowSelectedFilePaths] = useState<Record<string, string[]>>({});
  const [activeTab, setActiveTab] = useState("credentials");

  const xingchenProducts = useMemo(
    () =>
      bindableProducts.filter(
        (p) =>
          (p.productType === "xingchen-agent" || p.productType === "xingchen-workflow") &&
          p.enabled &&
          !p.revoked,
      ),
    [bindableProducts],
  );

  const selectedAgent = useMemo(
    () => agents.find((agent) => agent.id === selectedAgentId) ?? null,
    [agents, selectedAgentId],
  );

  const selectedWorkflowFields = useMemo(
    () =>
      selectedAgent
        ? workflowFieldsFromMapping(selectedAgent.requestMappingJson, DEFAULT_WORKFLOW_INPUT_KEY)
        : [],
    [selectedAgent],
  );

  const selectedWorkflowUsesFallbackSchema = useMemo(
    () =>
      selectedAgent?.protocolType === "xingchen_workflow_v1" &&
      selectedWorkflowFields.length === 1 &&
      selectedWorkflowFields[0]?.key === DEFAULT_WORKFLOW_INPUT_KEY,
    [selectedAgent, selectedWorkflowFields],
  );
  const selectedWorkflowUsesStreaming = false;

  function parseMappingValue(jsonText: string, keys: string[], fallback = "") {
    try {
      const value = JSON.parse(jsonText || "{}") as Record<string, unknown>;
      for (const key of keys) {
        const next = value[key];
        if (typeof next === "string" && next.trim()) return next;
      }
    } catch {
      return fallback;
    }
    return fallback;
  }

  function normalizeAgentWorkflowFields(values: Record<string, any>): WorkflowInputField[] {
    const rawFields = Array.isArray(values.workflowInputFields) ? values.workflowInputFields : [];
    return normalizeWorkflowInputFields(
      rawFields.map((field: WorkflowInputField, index: number) => ({
        ...field,
        key: String(field.key || "").trim(),
        label: String(field.label || field.key || "").trim(),
        order: index,
        required: field.required !== false,
        options: typeof (field.options as unknown) === "string"
          ? String(field.options).split(",").map((value) => value.trim()).filter(Boolean).map((value) => ({ label: value, value }))
          : field.options,
      })),
      String(values.inputParameter || DEFAULT_WORKFLOW_INPUT_KEY),
    );
  }

  function buildAgentWorkflowRequestMapping(values: Record<string, any>) {
    const fields = normalizeAgentWorkflowFields(values);
    const inputParameter =
      fields.find((field) => field.type === "multiline" || field.type === "string")?.key ??
      fields[0]?.key ??
      DEFAULT_WORKFLOW_INPUT_KEY;
    return buildWorkflowRequestMapping(fields, inputParameter);
  }

  function openCreateAgent() {
    setEditingAgentId(null);
    agentForm.resetFields();
    setAgentOpen(true);
  }

  function openEditAgent(row: ExternalAgentConfig) {
    setEditingAgentId(row.id);
    agentForm.setFieldsValue({
      productId: row.productId,
      name: row.name,
      endpoint: row.endpoint,
      agentId: row.agentId ?? undefined,
      botId: row.botId ?? undefined,
      flowId: row.flowId ?? undefined,
      protocolType: row.protocolType,
      authenticationType: row.authenticationType,
      credentialId: row.credentialId ?? undefined,
      streamingType: row.streamingType,
      requestMappingJson: row.requestMappingJson,
      responseMappingJson: row.responseMappingJson,
      sessionMappingJson: row.sessionMappingJson,
      errorMappingJson: row.errorMappingJson,
      inputParameter: parseMappingValue(row.requestMappingJson, ["inputParameter", "input_parameter"], "AGENT_USER_INPUT"),
      workflowInputFields: workflowFieldsFromMapping(
        row.requestMappingJson,
        parseMappingValue(row.requestMappingJson, ["inputParameter", "input_parameter"], "AGENT_USER_INPUT"),
      ),
      responseTextField: parseMappingValue(row.responseMappingJson, ["textField", "text_field"], "answer"),
      mockMode: row.mockMode,
      enabled: row.enabled,
    });
    setAgentOpen(true);
  }

  async function refreshAll() {
    setLoading(true);
    try {
      const [credRows, agentRows, bindableRows, usageRows] = await Promise.allSettled([
        credentialApi.list(),
        externalAgentApi.list(),
        externalAgentApi.listBindableProducts(),
        externalAgentApi.listUsage(),
      ]);
      if (credRows.status === "fulfilled") setCredentials(credRows.value);
      else message.warning(`凭据列表加载失败：${String(credRows.reason)}`);

      if (agentRows.status === "fulfilled") {
        setAgents(agentRows.value);
        if (!selectedAgentId && agentRows.value.length > 0) setSelectedAgentId(agentRows.value[0].id);
      } else {
        message.warning(`智能体列表加载失败：${String(agentRows.reason)}`);
      }

      if (bindableRows.status === "fulfilled") setBindableProducts(bindableRows.value);
      else message.warning(`可绑定星辰商品加载失败：${String(bindableRows.reason)}`);

      if (usageRows.status === "fulfilled") setUsage(usageRows.value);
      else message.warning(`调用记录加载失败：${String(usageRows.reason)}`);
    } finally {
      setLoading(false);
    }
  }

  async function loadRuntimeDataDirectory(signal?: { cancelled: boolean }) {
    setRuntimeDirLoading(true);
    setRuntimeDirError(null);
    try {
      const info = await withTimeout(runtimeApi.getDataDirectory(), 5000, "读取运行数据目录超时");
      if (signal?.cancelled) return;
      setRuntimeDir(info);
    } catch (err) {
      if (signal?.cancelled) return;
      setRuntimeDir(null);
      setRuntimeDirError(String(err));
    } finally {
      if (!signal?.cancelled) setRuntimeDirLoading(false);
    }
  }

  useEffect(() => {
    void refreshAll();
  }, []);

  useEffect(() => {
    const signal = { cancelled: false };
    void loadRuntimeDataDirectory(signal);
    return () => {
      signal.cancelled = true;
    };
  }, []);

  useEffect(() => {
    workflowForm.resetFields();
    workflowForm.setFieldsValue({ workflowValues: workflowInitialValues(selectedWorkflowFields) });
    setWorkflowResult(null);
    setWorkflowPreviewJson("");
    setWorkflowFileNames({});
    setWorkflowSelectedFilePaths({});
  }, [selectedAgentId, selectedWorkflowFields, workflowForm]);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;
    listen<AgentStreamEvent>("agent:stream", (event) => {
      const payload = event.payload;
      if (payload.sessionId !== selectedSessionId) return;
      if (requestIdRef.current && payload.requestId !== requestIdRef.current) return;
      if (payload.event === "started") {
        setStreaming(true);
        setStreamingText("");
        setStreamProgress(payload.progress ?? 0);
      }
      if (payload.event === "text_delta" && payload.delta) {
        setStreamingText((prev) => prev + payload.delta);
        if (typeof payload.progress === "number") setStreamProgress(payload.progress);
      }
      if (payload.event === "completed" || payload.event === "cancelled" || payload.event === "error") {
        setStreaming(false);
        setStreamProgress(payload.progress ?? null);
        requestIdRef.current = null;
        if (payload.event === "error") {
          message.error(payload.message || payload.errorCode || "智能体调用失败");
        }
        if (payload.event === "cancelled") {
          message.warning("已停止生成");
        }
        if (payload.done && selectedSessionId) {
          void loadMessages(selectedSessionId);
          void loadUsage();
        }
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [selectedSessionId]);

  useEffect(() => {
    if (!selectedAgentId) {
      setSessions([]);
      setSelectedSessionId(undefined);
      return;
    }
    void loadSessions(selectedAgentId);
  }, [selectedAgentId]);

  useEffect(() => {
    if (!selectedSessionId) {
      setMessages([]);
      return;
    }
    void loadMessages(selectedSessionId);
  }, [selectedSessionId]);

  async function loadSessions(agentId: string) {
    const rows = await externalAgentApi.listSessions(agentId);
    setSessions(rows);
    setSelectedSessionId((current) => current ?? rows[0]?.id);
  }

  async function loadMessages(sessionId: string) {
    const rows = await externalAgentApi.listMessages(sessionId);
    setMessages(rows);
  }

  async function loadUsage() {
    setUsage(await externalAgentApi.listUsage());
  }

  async function saveCredential() {
    const values = await credentialForm.validateFields();
    await credentialApi.create({
      provider: "xingchen",
      credentialType: values.credentialType || "app_key_secret",
      label: values.label,
      ownerScope: "local-user",
      secrets: {
        appId: values.appId || null,
        apiKey: values.apiKey || null,
        apiSecret: values.apiSecret || null,
        bearerToken: values.bearerToken || null,
      },
    });
    credentialForm.resetFields();
    setCredentialOpen(false);
    message.success("凭据已保存，前端不会再读取明文");
    await refreshAll();
  }

  async function saveAgent() {
    const values = await agentForm.validateFields();
    const payload = {
      productId: values.productId,
      name: values.name,
      endpoint: values.protocolType === "xingchen_workflow_v1" ? XINGCHEN_WORKFLOW_V1_ENDPOINT : values.endpoint,
      agentId: values.agentId || null,
      botId: values.botId || null,
      flowId: values.flowId || null,
      protocolType: values.protocolType || "configurable",
      authenticationType: values.authenticationType,
      credentialId: values.credentialId || null,
      streamingType: values.streamingType,
      requestMappingJson: values.protocolType === "xingchen_workflow_v1"
        ? buildAgentWorkflowRequestMapping(values)
        : values.requestMappingJson || "{}",
      responseMappingJson: values.protocolType === "xingchen_workflow_v1"
        ? JSON.stringify({ textField: values.responseTextField || "answer" })
        : values.responseMappingJson || "{}",
      sessionMappingJson: values.sessionMappingJson || "{}",
      errorMappingJson: values.errorMappingJson || "{}",
      mockMode: values.mockMode,
      enabled: values.enabled,
    };
    if (editingAgentId) {
      await externalAgentApi.update(editingAgentId, payload);
    } else {
      await externalAgentApi.create(payload);
    }
    agentForm.resetFields();
    setEditingAgentId(null);
    setAgentOpen(false);
    message.success(editingAgentId ? "智能体配置已更新" : "智能体配置已创建");
    await refreshAll();
  }

  function importWorkflowSchemaIntoAgentForm() {
    const text = String(agentForm.getFieldValue("workflowSchemaImport") || "");
    const fields = importWorkflowFieldsFromText(text);
    if (fields.length === 0) {
      message.warning("未能识别字段，请手动添加并核对星辰开始节点字段名。");
      return;
    }
    const inputParameter =
      fields.find((field) => field.type === "multiline" || field.type === "string")?.key ??
      fields[0]?.key ??
      DEFAULT_WORKFLOW_INPUT_KEY;
    agentForm.setFieldsValue({ workflowInputFields: fields, inputParameter });
    message.success(`已导入 ${fields.length} 个字段`);
  }

  function applyXingchenWorkflowBasicPreset() {
    agentForm.setFieldsValue({
      protocolType: "xingchen_workflow_v1",
      endpoint: XINGCHEN_WORKFLOW_V1_ENDPOINT,
      authenticationType: "bearer",
      streamingType: "none",
      mockMode: false,
      inputParameter: "AGENT_USER_INPUT",
      workflowInputFields: [{
        key: "AGENT_USER_INPUT",
        label: "用户输入",
        type: "multiline",
        required: true,
        placeholder: "请输入要发给工作流开始节点的内容",
        description: "对应星辰开始节点变量 AGENT_USER_INPUT",
      }],
      responseTextField: "file_content",
    });
    message.success("已套用：AGENT_USER_INPUT，同步调用，输出 file_content/file_name。");
  }

  async function testAgent(agentId: string) {
    const agent = agents.find((row) => row.id === agentId);
    if (agent?.protocolType === "xingchen_workflow_v1" && !agent.mockMode) {
      const confirmed = await new Promise<boolean>((resolve) => {
        Modal.confirm({
          title: "确认执行真实连接测试？",
          content: "将使用你保存的 BYOK 凭据向讯飞 Workflow Open API v1 发送最小测试输入“你好”，可能消耗少量讯飞额度。API Key/API Secret 不会显示或写入日志。",
          okText: "确认测试",
          cancelText: "取消",
          onOk: () => resolve(true),
          onCancel: () => resolve(false),
        });
      });
      if (!confirmed) return;
    }
    const result = await externalAgentApi.testConnection(agentId);
    if (result.ok) {
      message.success(result.message);
    } else {
      Modal.error({
        title: "讯飞 Workflow 连接测试失败",
        width: 680,
        content: (
          <Space direction="vertical" size={8} className="w-full">
            <Paragraph>{result.message}</Paragraph>
            {result.errorCode && <Paragraph>错误码：{result.errorCode}</Paragraph>}
            {result.httpStatus && <Paragraph>HTTP status：{result.httpStatus}</Paragraph>}
            {result.requestId && (
              <Paragraph copyable={{ text: result.requestId }}>
                请求 ID：{result.requestId}
              </Paragraph>
            )}
            <Paragraph type="secondary">
              以上信息已脱敏，不包含 API Key、API Secret 或 Authorization Header。
            </Paragraph>
          </Space>
        ),
      });
    }
    await refreshAll();
  }

  async function createSession() {
    if (!selectedAgentId) return;
    const session = await externalAgentApi.createSession({
      externalAgentId: selectedAgentId,
      title: "智能体对话",
    });
    setSelectedSessionId(session.id);
    await loadSessions(selectedAgentId);
  }

  async function sendMessage() {
    if (!selectedSessionId || !input.trim() || streaming) return;
    const content = input.trim();
    setInput("");
    setStreamingText("");
    setStreamProgress(null);
    const result = await externalAgentApi.sendMessage({
      sessionId: selectedSessionId,
      content,
      scenario: mockScenario === "normal" ? null : mockScenario,
    });
    requestIdRef.current = result.requestId;
    setStreaming(true);
    await loadMessages(selectedSessionId);
  }

  async function stopGeneration() {
    if (!requestIdRef.current) return;
    await externalAgentApi.cancelRequest(requestIdRef.current);
  }

  async function chooseWorkflowFiles(field: WorkflowInputField) {
    const extensions = field.fileConfig?.allowedExtensions?.map((ext) => ext.replace(/^\./, "")).filter(Boolean) ?? [];
    const picked = await openDialog({
      multiple: field.type === "files",
      directory: false,
      filters: extensions.length > 0 ? [{ name: field.label || field.key, extensions }] : undefined,
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    setWorkflowSelectedFilePaths((prev) => ({ ...prev, [field.key]: paths }));
    setWorkflowFileNames((prev) => ({
      ...prev,
      [field.key]: paths.map((path) => path.split(/[\\/]/).pop() || "file"),
    }));
  }

  async function invokeWorkflowForm() {
    if (!selectedAgent || workflowSubmitting) return;
    setWorkflowSubmitting(true);
    try {
      const values = await workflowForm.validateFields();
      const { parameters, filePaths } = buildWorkflowSubmission(
        selectedWorkflowFields,
        { ...(values.workflowValues ?? {}), ...workflowSelectedFilePaths },
      );
      if (Object.keys(parameters).length === 0 && Object.keys(filePaths).length === 0) {
        throw new Error("请至少填写一个 Workflow 输入项，否则星辰会收到空 parameters。");
      }
      const preview = workflowPreview(selectedWorkflowFields, parameters, filePaths);
      setWorkflowPreviewJson(JSON.stringify({
        flow_id: selectedAgent.flowId ? "已配置（脱敏）" : "未配置",
        parameters: preview,
        stream: selectedWorkflowUsesStreaming,
        note: selectedWorkflowUsesStreaming ? "后端按流式协议调用并聚合为一次结果" : "后端按非流式协议调用",
      }, null, 2));
      const result = await externalAgentApi.invokeWorkflow({
        externalAgentId: selectedAgent.id,
        parameters,
        filePaths,
        sourceFeature: "dynamic-workflow-form",
      });
      setWorkflowResult(result);
      await refreshAll();
      if (result.ok) {
        message.success(result.mock ? "Mock Workflow 调用完成" : "讯飞 Workflow 调用完成");
      } else {
        Modal.error({
          title: "Workflow 调用失败",
          width: 680,
          content: (
            <Space direction="vertical" className="w-full">
              <Paragraph>{result.message}</Paragraph>
              {result.code != null && <Text>错误码：{result.code}</Text>}
              {result.httpStatus != null && <Text>HTTP status：{result.httpStatus}</Text>}
              {result.remoteId && <Text copyable={{ text: result.remoteId }}>请求 ID：{result.remoteId}</Text>}
            </Space>
          ),
        });
      }
    } catch (err) {
      const detail = normalizeWorkflowException(err);
      const fallbackResult: AgentWorkflowInvokeResult = {
        ok: false,
        externalAgentId: selectedAgent.id,
        requestId: "",
        remoteId: null,
        content: "",
        progress: null,
        usage: null,
        httpStatus: null,
        code: null,
        message: detail,
        mock: selectedAgent.mockMode,
        outputFiles: [],
        debugJson: null,
      };
      setWorkflowResult(fallbackResult);
      Modal.error({
        title: "Workflow 调用未完成",
        width: 680,
        content: (
          <Space direction="vertical" className="w-full">
            <Paragraph>{detail}</Paragraph>
            <Paragraph type="secondary">
              如果星辰返回 20354 或 schema error，请回到“智能体”页编辑该智能体的“Workflow 输入字段”，
              字段 key 必须和星辰工作流开始节点参数名完全一致。
            </Paragraph>
          </Space>
        ),
      });
    } finally {
      setWorkflowSubmitting(false);
    }
  }

  function renderWorkflowInput(field: WorkflowInputField) {
    const labelText = field.label || field.key;
    const label = (
      <Space size={6}>
        <span>{labelText}</span>
        {field.key !== labelText && <Text type="secondary">参数：{field.key}</Text>}
      </Space>
    );
    const rules = field.required ? [{ required: true, message: `${field.label || field.key} 为必填字段` }] : undefined;
    if (field.type === "boolean") {
      return (
        <Form.Item key={field.key} name={["workflowValues", field.key]} label={label} valuePropName="checked">
          <Checkbox>{field.description || "启用"}</Checkbox>
        </Form.Item>
      );
    }
    if (field.type === "integer" || field.type === "number") {
      return (
        <Form.Item key={field.key} name={["workflowValues", field.key]} label={label} rules={rules} extra={field.description}>
          <InputNumber className="w-full" precision={field.type === "integer" ? 0 : undefined} placeholder={field.placeholder} />
        </Form.Item>
      );
    }
    if (field.type === "select") {
      return (
        <Form.Item key={field.key} name={["workflowValues", field.key]} label={label} rules={rules} extra={field.description}>
          <Select
            placeholder={field.placeholder}
            options={(field.options ?? []).map((option) => ({ label: option.label, value: option.value }))}
          />
        </Form.Item>
      );
    }
    if (field.type === "json" || field.type === "multiline") {
      return (
        <Form.Item key={field.key} name={["workflowValues", field.key]} label={label} rules={rules} extra={field.description}>
          <Input.TextArea rows={field.type === "json" ? 4 : 3} placeholder={field.placeholder} />
        </Form.Item>
      );
    }
    if (field.type === "file" || field.type === "files") {
      return (
        <Form.Item key={field.key} label={label} required={field.required} extra={field.description || "文件会由 Rust 后端上传，本地绝对路径不会发送给星辰。"}>
          <Space direction="vertical" className="w-full">
            <Button onClick={() => chooseWorkflowFiles(field)}>
              {field.type === "files" ? "选择多个文件" : "选择文件"}
            </Button>
            <Text type="secondary">
              {(workflowFileNames[field.key] ?? []).length > 0
                ? (workflowFileNames[field.key] ?? []).join("、")
                : "尚未选择文件"}
            </Text>
          </Space>
        </Form.Item>
      );
    }
    return (
      <Form.Item key={field.key} name={["workflowValues", field.key]} label={label} rules={rules} extra={field.description}>
        <Input placeholder={field.placeholder} />
      </Form.Item>
    );
  }

  const credentialTab = (
    <Space direction="vertical" size={16} className="w-full">
      <Alert
        showIcon
        type="info"
        message="BYOK 凭据只由 Rust 后端保存和读取"
        description="SQLite 只保存 provider、label、secretReference、maskedHint 等元数据；前端无法读取完整 APPID/API Key/API Secret/Token。"
      />
      <Button
        type="primary"
        icon={<KeyRound size={16} />}
        onClick={() => {
          credentialForm.resetFields();
          credentialForm.setFieldsValue({ credentialType: "app_key_secret" });
          setCredentialOpen(true);
        }}
      >
        新增讯飞星辰Workflow凭据
      </Button>
      <Table
        loading={loading}
        rowKey="id"
        dataSource={credentials}
        columns={[
          { title: "名称", dataIndex: "label" },
          { title: "Provider", dataIndex: "provider" },
          { title: "类型", dataIndex: "credentialType" },
          {
            title: "状态",
            render: (_, row) => row.configured ? <Tag color="green">已配置 {row.maskedHint}</Tag> : <Tag>未配置</Tag>,
          },
          { title: "更新时间", dataIndex: "updatedAt" },
          {
            title: "操作",
            render: (_, row) => (
              <Space>
                <Button size="small" onClick={async () => {
                  const usageRows = await credentialApi.getUsage(row.id);
                  Modal.info({
                    title: "凭据引用",
                    content: usageRows.length ? (
                      <List dataSource={usageRows} renderItem={(u) => (
                        <List.Item>{u.agentName} / {u.productName}</List.Item>
                      )} />
                    ) : "当前没有智能体引用该凭据",
                  });
                }}>引用</Button>
                <Popconfirm title="删除凭据？被引用时需要先解绑或强制失效。" onConfirm={async () => {
                  try {
                    await credentialApi.delete(row.id, false);
                  } catch (err) {
                    message.warning(String(err));
                    return;
                  }
                  await refreshAll();
                }}>
                  <Button size="small" danger>删除</Button>
                </Popconfirm>
              </Space>
            ),
          },
        ]}
      />
    </Space>
  );

  const agentTab = (
    <Space direction="vertical" size={16} className="w-full">
      <Alert
        showIcon
        type="warning"
        message="真实星辰调用需要按发布页面文档配置"
        description="Mock 模式可完整演示连接、流式、取消、错误映射和调用记录；讯飞星辰 Workflow Open API v1 使用官方固定协议，只需要绑定商品、选择凭据并填写 Flow ID。"
      />
      <Button type="primary" icon={<Bot size={16} />} onClick={openCreateAgent}>
        创建智能体配置
      </Button>
      <Table
        loading={loading}
        rowKey="id"
        dataSource={agents}
        columns={[
          { title: "名称", dataIndex: "name" },
          { title: "商品", dataIndex: "productName" },
          { title: "Endpoint", dataIndex: "endpoint", ellipsis: true },
          {
            title: "模式",
            render: (_, row) => row.mockMode ? <Tag color="blue">Mock演示</Tag> : <Tag color="gold">真实Provider</Tag>,
          },
          {
            title: "协议",
            render: (_, row) => row.protocolType === "xingchen_workflow_v1" ? <Tag color="purple">Workflow v1</Tag> : <Tag>通用配置</Tag>,
          },
          {
            title: "启用",
            render: (_, row) => row.enabled ? <Tag color="green">启用</Tag> : <Tag>禁用</Tag>,
          },
          {
            title: "操作",
            render: (_, row) => (
              <Space>
                <Button size="small" onClick={() => testAgent(row.id)}>测试连接</Button>
                <Button size="small" onClick={() => setSelectedAgentId(row.id)}>对话</Button>
                <Button size="small" onClick={() => openEditAgent(row)}>编辑</Button>
                <Popconfirm
                  title="删除该智能体配置？"
                  description="配置会从列表和插件下拉框中移除，历史会话和调用记录会保留；不会删除凭据。"
                  okText="删除配置"
                  cancelText="取消"
                  onConfirm={async () => {
                    try {
                      await externalAgentApi.delete(row.id);
                      if (selectedAgentId === row.id) {
                        setSelectedAgentId(undefined);
                        setSelectedSessionId(undefined);
                        setSessions([]);
                        setMessages([]);
                      }
                      message.success("智能体配置已删除");
                      await refreshAll();
                    } catch (err) {
                      message.error(`删除失败：${String(err)}`);
                    }
                  }}
                >
                  <Button size="small" danger>删除配置</Button>
                </Popconfirm>
              </Space>
            ),
          },
        ]}
      />
    </Space>
  );

  const chatTab = (
    <div className="grid grid-cols-[280px_1fr] gap-4 min-h-[560px]">
      <Card title="智能体与会话" size="small">
        <Select
          className="w-full mb-3"
          placeholder="选择智能体"
          value={selectedAgentId}
          onChange={setSelectedAgentId}
          options={agents.map((a) => ({ value: a.id, label: `${a.name}${a.mockMode ? "（Mock）" : ""}` }))}
        />
        <Button block onClick={createSession} disabled={!selectedAgentId}>新建会话</Button>
        <Divider />
        <List
          size="small"
          dataSource={sessions}
          renderItem={(session) => (
            <List.Item
              className={session.id === selectedSessionId ? "bg-blue-50 rounded px-2" : "px-2"}
              onClick={() => setSelectedSessionId(session.id)}
              style={{ cursor: "pointer" }}
            >
              <MessageSquare size={14} /> <span className="ml-2">{session.title}</span>
            </List.Item>
          )}
        />
      </Card>
      <div className="grid grid-cols-[minmax(0,1fr)_360px] gap-4">
        <Card
          title="智能体对话"
          extra={<Tag color={selectedAgent?.mockMode ? "blue" : "gold"}>
            {streaming && streamProgress !== null
              ? `进度 ${Math.round(streamProgress * 100)}%`
              : selectedAgent?.mockMode ? "Mock演示" : "真实Provider"}
          </Tag>}
        >
          {selectedAgent && (selectedAgent.protocolType === "xingchen_workflow_v1" || selectedAgent.mockMode) && (
            <Card
              size="small"
              className="mb-3"
              title="调用当前 Workflow"
              extra={
                <Tag color={selectedAgent.mockMode ? "blue" : "purple"}>
                  {selectedAgent.mockMode ? "Mock" : selectedWorkflowUsesStreaming ? "stream=true，后端聚合" : "stream=false"}
                </Tag>
              }
            >
              <div className="mb-3 rounded-lg border border-orange-100 bg-orange-50 px-3 py-2 text-sm text-orange-900">
                填写下面的输入项后点击调用。真实请求由后端使用安全凭据发送，密钥不会显示在页面里。
              </div>
              {selectedWorkflowUsesFallbackSchema && !selectedAgent.mockMode && (
                <div className="mb-3 flex flex-col gap-2 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-900 md:flex-row md:items-center md:justify-between">
                  <span>
                    当前使用默认参数 <Text code>AGENT_USER_INPUT</Text>。如果星辰开始节点不是这个名字，请先修改字段，否则可能报 20354。
                  </span>
                  <Button size="small" onClick={() => {
                    setActiveTab("agents");
                    openEditAgent(selectedAgent);
                  }}>
                    修改字段
                  </Button>
                </div>
              )}
              <Form form={workflowForm} layout="vertical">
                <div className="grid grid-cols-1 md:grid-cols-2 gap-x-3">
                  {selectedWorkflowFields.map(renderWorkflowInput)}
                </div>
              </Form>
              <Space className="mb-3" wrap>
                <Button type="primary" loading={workflowSubmitting} onClick={invokeWorkflowForm}>
                  调用 Workflow
                </Button>
                <Text type="secondary">输入项会按字段名生成 parameters。</Text>
              </Space>
              {workflowPreviewJson && (
                <details className="mb-3 rounded-lg border border-gray-200 bg-gray-50 px-3 py-2">
                  <summary className="cursor-pointer text-sm text-gray-600">查看本次请求预览（已脱敏）</summary>
                  <Input.TextArea className="mt-2" rows={5} readOnly value={workflowPreviewJson} />
                </details>
              )}
              {workflowResult && (
                <Alert
                  type={workflowResult.ok ? "success" : "error"}
                  showIcon
                  message={workflowResult.message}
                  description={(
                    <Space direction="vertical" className="w-full">
                      {workflowResult.remoteId && <Text copyable={{ text: workflowResult.remoteId }}>请求 ID：{workflowResult.remoteId}</Text>}
                      {workflowResult.httpStatus != null && <Text>HTTP status：{workflowResult.httpStatus}</Text>}
                      {(workflowResult.outputFiles ?? []).map((file) => (
                        <Text key={file.path} copyable={{ text: file.path }}>
                          生成文件：{file.fileName}（{Math.max(1, Math.round(file.size / 1024))} KB） - {file.path}
                        </Text>
                      ))}
                      {workflowResult.content && <div className="ai-markdown"><Markdown>{workflowResult.content}</Markdown></div>}
                    </Space>
                  )}
                />
              )}
            </Card>
          )}
          <div className="h-[380px] overflow-auto rounded border border-gray-100 p-4 bg-white">
            {messages.map((m) => (
              <div key={m.id} className={`mb-3 ${m.role === "user" ? "text-right" : "text-left"}`}>
                <div className={`inline-block max-w-[78%] rounded-2xl px-4 py-2 ${m.role === "user" ? "bg-blue-600 text-white" : "bg-gray-100 text-gray-800"}`}>
                  {m.role === "user" ? (
                    <div className="whitespace-pre-wrap">{m.content || (m.status === "streaming" ? "生成中..." : "")}</div>
                  ) : (
                    <div className="ai-markdown"><Markdown>{stripPlanningUpdateBlock(m.content || (m.status === "streaming" ? "生成中..." : ""))}</Markdown></div>
                  )}
                </div>
              </div>
            ))}
            {streaming && streamingText && (
              <div className="mb-3 text-left">
                <div className="inline-block max-w-[78%] rounded-2xl px-4 py-2 bg-gray-100 text-gray-800">
                  <div className="ai-markdown"><Markdown>{stripPlanningUpdateBlock(streamingText)}</Markdown></div>
                </div>
              </div>
            )}
          </div>
          <Space className="mt-3 w-full" direction="vertical">
            <Select
              value={mockScenario}
              onChange={setMockScenario}
              options={[
                { value: "normal", label: "正常流式" },
                { value: "auth_failed", label: "模拟鉴权失败" },
                { value: "rate_limited", label: "模拟限流" },
                { value: "timeout", label: "模拟超时" },
                { value: "provider_error", label: "模拟Provider错误" },
              ]}
            />
            <Input.TextArea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              rows={3}
              placeholder="本轮仅支持文本。文件、图片、知识库和数据库绑定将在阶段5接入。"
            />
            <Space>
              <Button type="primary" onClick={sendMessage} disabled={!selectedSessionId || streaming}>
                发送
              </Button>
              <Button onClick={stopGeneration} disabled={!streaming}>
                停止生成
              </Button>
            </Space>
          </Space>
        </Card>
        <PlanningWithFilesPanel
          sessionKind="agent"
          sessionId={selectedSessionId ?? null}
          disabled={streaming}
        />
      </div>
    </div>
  );

  const usageTab = (
    <Table
      rowKey="id"
      dataSource={usage}
      columns={[
        { title: "请求ID", dataIndex: "requestId", ellipsis: true },
        { title: "状态", dataIndex: "status" },
        { title: "错误码", dataIndex: "providerErrorCode" },
        { title: "耗时(ms)", dataIndex: "durationMs" },
        { title: "输入估算", dataIndex: "estimatedInputUsage" },
        { title: "输出估算", dataIndex: "estimatedOutputUsage" },
        {
          title: "来源插件",
          dataIndex: "sourcePluginId",
          render: (value) => value ? <Tag color="blue">{value}</Tag> : <Tag>直接调用</Tag>,
        },
        { title: "开始时间", dataIndex: "startedAt" },
      ]}
    />
  );

  return (
    <div className="h-full overflow-auto p-8 bg-gradient-to-br from-slate-50 via-blue-50 to-cyan-50">
      <Space direction="vertical" size={20} className="w-full">
        <div>
          <Title level={2} className="!mb-1">AI资源中心</Title>
          <Paragraph type="secondary">
            管理 BYOK 凭据、星辰智能体配置、会话和调用记录。这里不会保存或显示完整密钥。
          </Paragraph>
        </div>
        <Alert
          showIcon
          icon={<ShieldCheck size={18} />}
          message="安全边界"
          description="插件只能请求使用 credentialId，真实密钥只在 Rust 后端安全存储中解密使用；所有网络调用由后端执行，并经过 endpoint 安全策略检查。"
        />
        <Alert
          type="info"
          showIcon
          message="当前运行数据目录"
          description={
            runtimeDirLoading ? (
              "正在读取当前数据目录..."
            ) : runtimeDirError ? (
              <Space direction="vertical" size={6}>
                <span>无法读取当前数据目录：{runtimeDirError}</span>
                <Button size="small" onClick={() => loadRuntimeDataDirectory()}>
                  重试
                </Button>
              </Space>
            ) : runtimeDir ? (
              <Space direction="vertical" size={2}>
                <span>{runtimeDir.path}</span>
                <span>来源：{runtimeDir.source}</span>
                <span>目录存在：{runtimeDir.exists ? "是" : "否"}</span>
                <span>可写：{runtimeDir.writable ? "是" : "否"}</span>
                <span>数据库：{runtimeDir.databasePath}</span>
              </Space>
            ) : (
              "未读取到当前数据目录"
            )
          }
        />
        <Tabs
          activeKey={activeTab}
          onChange={setActiveTab}
          items={[
            { key: "credentials", label: "凭据", icon: <KeyRound size={16} />, children: credentialTab },
            { key: "agents", label: "智能体", icon: <Bot size={16} />, children: agentTab },
            { key: "chat", label: "会话", icon: <MessageSquare size={16} />, children: chatTab },
            { key: "usage", label: "调用记录", icon: <RadioTower size={16} />, children: usageTab },
          ]}
        />
      </Space>

      <Modal title="新增讯飞星辰Workflow凭据" open={credentialOpen} onCancel={() => setCredentialOpen(false)} onOk={saveCredential} destroyOnHidden>
        <Alert className="mb-4" type="info" showIcon message="保存后表单会清空，列表只显示脱敏尾号。" />
        <Form form={credentialForm} layout="vertical" initialValues={{ credentialType: "app_key_secret" }}>
          <Form.Item name="label" label="名称" rules={[{ required: true }]}>
            <Input placeholder="例如：我的星辰测试凭据" />
          </Form.Item>
          <Form.Item name="credentialType" label="凭据类型" rules={[{ required: true }]}>
            <Select disabled options={[
              { value: "app_key_secret", label: "讯飞星辰Workflow凭据（APPID + API Key + API Secret）" },
            ]} />
          </Form.Item>
          <Form.Item name="appId" label="APPID" rules={[{ required: true }]}>
            <Input.Password autoComplete="off" />
          </Form.Item>
          <Form.Item name="apiKey" label="API Key" rules={[{ required: true }]}>
            <Input.Password autoComplete="off" />
          </Form.Item>
          <Form.Item name="apiSecret" label="API Secret" rules={[{ required: true }]}>
            <Input.Password autoComplete="off" />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={editingAgentId ? "编辑智能体配置" : "创建智能体配置"}
        open={agentOpen}
        onCancel={() => {
          setAgentOpen(false);
          setEditingAgentId(null);
          agentForm.resetFields();
        }}
        onOk={saveAgent}
        width={760}
        destroyOnHidden
      >
        <Form
          form={agentForm}
          layout="vertical"
          initialValues={{
            endpoint: "mock://xingchen",
            protocolType: "configurable",
            authenticationType: "none",
            streamingType: "sse",
            requestMappingJson: "{}",
            responseMappingJson: "{}",
            sessionMappingJson: "{}",
            errorMappingJson: "{}",
            inputParameter: "AGENT_USER_INPUT",
            workflowInputFields: [{
              key: "AGENT_USER_INPUT",
              label: "用户输入",
              type: "multiline",
              required: true,
              placeholder: "请输入工作流开始节点文本",
            }],
            mockMode: true,
            enabled: true,
          }}
          onValuesChange={(changed) => {
            if (changed.protocolType === "xingchen_workflow_v1") {
              agentForm.setFieldsValue({
                endpoint: XINGCHEN_WORKFLOW_V1_ENDPOINT,
                authenticationType: "bearer",
                streamingType: "none",
                mockMode: false,
                inputParameter: "AGENT_USER_INPUT",
                workflowInputFields: [{
                  key: "AGENT_USER_INPUT",
                  label: "用户输入",
                  type: "multiline",
                  required: true,
                  placeholder: "请输入工作流开始节点文本",
                }],
                responseTextField: "file_content",
              });
            }
          }}
        >
          <Form.Item name="productId" label="绑定已安装星辰商品" rules={[{ required: true }]}>
            <Select
              placeholder="请先在 AI应用市场 获取、安装并启用星辰商品"
              options={xingchenProducts.map((p) => ({ value: p.id, label: `${p.name} / ${p.currentVersion}` }))}
            />
          </Form.Item>
          <Form.Item name="name" label="配置名称" rules={[{ required: true }]}>
            <Input placeholder="例如：星辰学习助手 Mock" />
          </Form.Item>
          <Form.Item name="protocolType" label="协议类型" rules={[{ required: true }]}>
            <Select options={[
              { value: "configurable", label: "通用配置模式（Mock/预留）" },
              { value: "xingchen_workflow_v1", label: "讯飞星辰 Workflow Open API v1" },
            ]} />
          </Form.Item>
          {isWorkflowV1 && (
            <Alert
              className="mb-3"
              type="warning"
              showIcon
              message="真实 Workflow 调用说明"
              description="Endpoint、Bearer API_KEY:API_SECRET、开始节点 parameters 和 stream 均由 Rust 后端按官方协议构造；生成 Word/文件的工作流建议选择 none（同步），聊天类工作流可选择 sse。"
            />
          )}
          {!isWorkflowV1 && (
            <Form.Item name="mockMode" label="Mock Provider" valuePropName="checked">
              <Switch />
            </Form.Item>
          )}
          <Form.Item name="endpoint" label="Endpoint" rules={[{ required: true }]}>
            <Input disabled={isWorkflowV1} placeholder="Mock 使用 mock://xingchen；Workflow v1 使用固定官方 Endpoint" />
          </Form.Item>
          {isWorkflowV1 ? (
            <>
              <Form.Item name="flowId" label="Flow ID" rules={[{ required: true }]}>
                <Input placeholder="填写讯飞星辰工作流发布页提供的 Flow ID" />
              </Form.Item>
              <Form.Item name="inputParameter" label="开始节点参数名" rules={[{ required: true }]}>
                <Input placeholder="AGENT_USER_INPUT / question / input" />
              </Form.Item>
              <Form.Item
                name="workflowSchemaImport"
                label="导入 JSON Schema / 字段 JSON / YAML（可选）"
                extra="不会远程读取星辰 schema；请从星辰编排页核对字段名后粘贴。"
              >
                <Input.TextArea rows={3} />
              </Form.Item>
              <Space className="mb-3" wrap>
                <Button onClick={applyXingchenWorkflowBasicPreset}>
                  套用当前截图配置
                </Button>
                <Button onClick={importWorkflowSchemaIntoAgentForm}>
                  导入字段
                </Button>
                <Text type="secondary">当前截图对应：开始节点参数 AGENT_USER_INPUT，结束节点返回 file_content / file_name。</Text>
              </Space>
              <Form.List name="workflowInputFields">
                {(fields, { add, remove }) => (
                  <Form.Item
                    label="Workflow 输入字段"
                    extra="字段 key 会原样写入 parameters；空的非必填字段不会发送。"
                  >
                    <Space direction="vertical" className="w-full" size={8}>
                      {fields.map((field) => (
                        <Card key={field.key} size="small">
                          <Space align="baseline" className="w-full" size={8} wrap>
                            <Form.Item
                              {...field}
                              name={[field.name, "key"]}
                              rules={[{ required: true, message: "字段名不能为空" }]}
                            >
                              <Input placeholder="major / learning_days" style={{ width: 180 }} />
                            </Form.Item>
                            <Form.Item {...field} name={[field.name, "label"]}>
                              <Input placeholder="显示名称" style={{ width: 130 }} />
                            </Form.Item>
                            <Form.Item {...field} name={[field.name, "type"]}>
                              <Select style={{ width: 145 }} options={WORKFLOW_FIELD_TYPE_OPTIONS} />
                            </Form.Item>
                            <Form.Item {...field} name={[field.name, "required"]} valuePropName="checked">
                              <Checkbox>必填</Checkbox>
                            </Form.Item>
                            <Form.Item {...field} name={[field.name, "defaultValue"]}>
                              <Input placeholder="默认值" style={{ width: 150 }} />
                            </Form.Item>
                            <Form.Item {...field} name={[field.name, "options"]}>
                              <Input placeholder="select选项，逗号分隔" style={{ width: 170 }} />
                            </Form.Item>
                            <Form.Item {...field} name={[field.name, "fileConfig", "allowedExtensions"]}>
                              <Input placeholder="文件扩展名 pdf,docx" style={{ width: 155 }} />
                            </Form.Item>
                            <Form.Item {...field} name={[field.name, "fileConfig", "maxSizeMb"]}>
                              <InputNumber min={1} max={200} placeholder="MB" style={{ width: 90 }} />
                            </Form.Item>
                            <Button disabled={fields.length <= 1} onClick={() => remove(field.name)}>
                              删除
                            </Button>
                          </Space>
                          <Form.Item {...field} name={[field.name, "description"]}>
                            <Input placeholder="字段说明" />
                          </Form.Item>
                        </Card>
                      ))}
                      <Button
                        type="dashed"
                        onClick={() => add({ key: "", label: "", type: "string", required: true })}
                      >
                        添加输入字段
                      </Button>
                    </Space>
                  </Form.Item>
                )}
              </Form.List>
              <Form.Item name="responseTextField" label="响应文本字段">
                <Input placeholder="file_content / answer / content / text / result" />
              </Form.Item>
              <Form.Item label="UID策略">
                <Input disabled value="firstwork 生成稳定匿名本地 UID；不会使用 API Key、用户名、手机号或本机绝对路径" />
              </Form.Item>
            </>
          ) : (
            <Space className="w-full" size={12}>
              <Form.Item name="agentId" label="agentId">
                <Input />
              </Form.Item>
              <Form.Item name="botId" label="botId">
                <Input />
              </Form.Item>
              <Form.Item name="flowId" label="flowId">
                <Input />
              </Form.Item>
            </Space>
          )}
          <Form.Item name="credentialId" label="凭据" rules={isWorkflowV1 ? [{ required: true }] : undefined}>
            <Select
              allowClear={!isWorkflowV1}
              placeholder={isWorkflowV1 ? "选择已保存的讯飞星辰Workflow凭据" : "可选"}
              options={credentials.map((c) => ({ value: c.id, label: `${c.label} ${c.maskedHint ?? ""}` }))}
            />
          </Form.Item>
          <Space className="w-full" size={12}>
            <Form.Item name="authenticationType" label="鉴权类型" rules={[{ required: true }]}>
              <Select disabled={isWorkflowV1} options={[
                { value: "none", label: "none" },
                { value: "bearer", label: "bearer" },
                { value: "api_key_header", label: "api_key_header" },
                { value: "signed_request", label: "signed_request（预留）" },
              ]} />
            </Form.Item>
            <Form.Item name="streamingType" label="流式类型" rules={[{ required: true }]}>
              <Select options={[
                { value: "none", label: "none（同步，适合生成文件/Word）" },
                { value: "sse", label: "sse（流式，适合聊天）" },
                { value: "websocket", label: "websocket（预留）" },
                { value: "chunked_json", label: "chunked_json（预留）" },
              ]} />
            </Form.Item>
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Space>
          {!isWorkflowV1 && (
            <>
              <Form.Item name="requestMappingJson" label="请求字段映射 JSON">
                <Input.TextArea rows={2} />
              </Form.Item>
              <Form.Item name="responseMappingJson" label="响应字段映射 JSON">
                <Input.TextArea rows={2} />
              </Form.Item>
              <Form.Item name="sessionMappingJson" label="会话字段映射 JSON">
                <Input.TextArea rows={2} />
              </Form.Item>
              <Form.Item name="errorMappingJson" label="错误字段映射 JSON">
                <Input.TextArea rows={2} />
              </Form.Item>
            </>
          )}
        </Form>
      </Modal>
    </div>
  );
}
