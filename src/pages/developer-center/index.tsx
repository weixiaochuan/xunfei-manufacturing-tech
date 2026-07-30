import { useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Col,
  Descriptions,
  Divider,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Row,
  Select,
  Space,
  Statistic,
  Table,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import { Boxes, ChartNoAxesCombined, PackagePlus, ShieldCheck } from "lucide-react";
import { developerApi, marketplaceApi } from "@/lib/api";
import {
  DEFAULT_WORKFLOW_INPUT_KEY,
  WORKFLOW_FIELD_TYPE_OPTIONS,
  buildWorkflowRequestMapping,
  importWorkflowFieldsFromText,
  normalizeWorkflowInputFields,
} from "@/lib/workflowSchema";
import {
  CommerceHero,
  CommerceStatusTag,
  DeliveryModeTag,
} from "@/components/marketplace/CommerceShell";
import type {
  AiServiceDeliveryMode,
  DeveloperDashboard,
  DeveloperProduct,
  DeveloperProductInput,
  DeveloperProductVersion,
  MarketplaceMockSession,
  MarketplacePackageReport,
  MarketplaceReviewStatus,
  PluginRuntimeKind,
  ProductType,
  WorkflowInputField,
} from "@/types";

const { Paragraph, Text, Title } = Typography;

const productTypeOptions: Array<{ value: ProductType; label: string; runtime: PluginRuntimeKind }> = [
  { value: "local-plugin", label: "本地功能插件", runtime: "declarative-ui" },
  { value: "prompt-pack", label: "Prompt/模板包", runtime: "prompt-pack" },
  { value: "declarative-ui" as ProductType, label: "声明式 UI", runtime: "declarative-ui" },
  { value: "xingchen-agent", label: "星辰智能体调用项", runtime: "xingchen-agent" },
  { value: "xingchen-workflow", label: "星辰工作流调用项", runtime: "xingchen-workflow" },
  { value: "xingchen-mcp", label: "星辰 MCP 调用项", runtime: "xingchen-mcp" },
  { value: "mcp-connector", label: "远程 MCP Server 调用项", runtime: "mcp-connector" },
  { value: "ppt-master-extension", label: "PPT Master 扩展", runtime: "ppt-extension" },
  { value: "learning-assistant-extension", label: "AI 助学扩展", runtime: "learning-extension" },
];

type DeveloperProductFormValues = DeveloperProductInput & {
  initialVersion?: string;
  endpoint?: string;
  inputParameter?: string;
  externalUrl?: string;
  flowIdProvidedByUser?: boolean;
  requestMethod?: string;
  requestBodySchema?: string;
  responseTextField?: string;
  workflowInputFields?: WorkflowInputField[];
  workflowSchemaImport?: string;
  streaming?: boolean;
  authenticationType?: string;
  serverUrl?: string;
  transport?: string;
  capabilities?: string;
  timeoutMs?: number;
};

const deliveryModeOptions: Array<{ value: AiServiceDeliveryMode; label: string }> = [
  { value: "byok", label: "星辰 Workflow API（用户自备授权）" },
  { value: "hosted-api", label: "开发者托管 HTTPS API（预留/Mock）" },
  { value: "remote-mcp", label: "远程 MCP Server" },
];

function buildServiceConfiguration(values: DeveloperProductFormValues): Record<string, unknown> | null {
  if (values.deliveryMode === "byok") {
    const fields = normalizeDeveloperWorkflowFields(values);
    const inputParameter = fields.find((field) => field.type === "multiline" || field.type === "string")?.key
      ?? fields[0]?.key
      ?? DEFAULT_WORKFLOW_INPUT_KEY;
    return {
      endpoint: { type: "string", default: "https://xingchen-api.xf-yun.com/workflow/v1/chat/completions", readOnly: true },
      credentialId: { type: "credential-reference", provider: "xingchen", required: true },
      flowId: { type: "string", required: true, secret: false },
      inputParameter: { type: "string", default: inputParameter },
      inputSchema: { type: "workflow-input-schema", fields },
      requestMappingJson: buildWorkflowRequestMapping(fields, inputParameter),
      responseTextField: { type: "string", default: values.responseTextField || "answer" },
      externalUrl: { type: "string", default: values.externalUrl?.trim() || "https://xingchen.xfyun.cn/" },
      flowIdProvidedByUser: values.flowIdProvidedByUser !== false,
      setupNotice: "API Key、API Secret 和 Flow ID 必须属于同一星辰应用授权。",
    };
  }
  if (values.deliveryMode === "hosted-api") {
    return {
      endpoint: { type: "string", default: values.endpoint?.trim() || "mock://hosted-api" },
      requestMethod: values.requestMethod || "POST",
      requestBodySchema: values.requestBodySchema || "{\"input\":\"string\"}",
      responseTextField: values.responseTextField || "data.text",
      streaming: Boolean(values.streaming),
      authenticationType: values.authenticationType || "bearer",
      externalUrl: { type: "string", default: values.externalUrl?.trim() || "" },
      credentialId: { type: "credential-reference", provider: "hosted-api", required: false },
      mockOnly: true,
    };
  }
  if (values.deliveryMode === "remote-mcp") {
    return {
      serverUrl: { type: "string", default: values.serverUrl?.trim() || "mock://remote-mcp" },
      transport: values.transport || "streamable-http",
      authenticationType: values.authenticationType || "none",
      capabilities: (values.capabilities || "tools").split(",").map((item) => item.trim()).filter(Boolean),
      timeoutMs: values.timeoutMs || 30_000,
      externalUrl: { type: "string", default: values.externalUrl?.trim() || "" },
      credentialId: { type: "credential-reference", provider: "remote-mcp", required: false },
      mockOnly: true,
    };
  }
  return null;
}

function normalizeDeveloperWorkflowFields(values: DeveloperProductFormValues): WorkflowInputField[] {
  const rawFields = Array.isArray(values.workflowInputFields) ? values.workflowInputFields : [];
  return normalizeWorkflowInputFields(
    rawFields.map((field, index) => ({
      ...field,
      key: String(field.key || "").trim(),
      label: String(field.label || field.key || "").trim(),
      type: field.type || "string",
      order: index,
      required: field.required !== false,
      options: typeof (field.options as unknown) === "string"
        ? String(field.options).split(",").map((value) => value.trim()).filter(Boolean).map((value) => ({ label: value, value }))
        : field.options,
    })),
    values.inputParameter || DEFAULT_WORKFLOW_INPUT_KEY,
  );
}

function cents(value: number | undefined) {
  return `¥${((value ?? 0) / 100).toFixed(2)}`;
}

function developerPriceText(product: Pick<DeveloperProduct, "deliveryMode" | "price">) {
  if (product.deliveryMode) {
    if (product.price.amount === 0) return "外部授权/免费连接器";
    return `外部参考价 ${cents(product.price.amount)}`;
  }
  if (product.price.amount === 0) return "免费";
  return `${cents(product.price.amount)} · 本地演示`;
}

function statusTag(status: MarketplaceReviewStatus) {
  return <CommerceStatusTag status={status} />;
}

function RoleSwitcher({ session, onChanged }: { session: MarketplaceMockSession | null; onChanged: () => void }) {
  void onChanged;
  if (!import.meta.env.DEV) return null;
  return (
    <Alert
      type="info"
      showIcon
      className="mb-4"
      message="本地一体化市场模式"
      description={
        <Space wrap>
          <Text>当前账号：{session?.displayName ?? "本地用户"}。</Text>
          <Text>同一用户可以上传插件、提交审核、浏览市场并安装使用；后续接入云端服务器后再由服务端账号体系控制权限。</Text>
        </Space>
      }
    />
  );
}

function ReportView({ report }: { report: MarketplacePackageReport | null }) {
  if (!report) return <Empty description="尚未上传商品包" />;
  return (
    <Card size="small" title="上传检查报告">
      <Descriptions size="small" column={2}>
        <Descriptions.Item label="状态"><Tag color={report.ok ? "green" : "red"}>{report.status}</Tag></Descriptions.Item>
        <Descriptions.Item label="包格式"><Tag color={report.packageFormat === "v3-firstwork-plugin" ? "blue" : "default"}>{report.packageFormat}</Tag></Descriptions.Item>
        <Descriptions.Item label="manifest">{report.manifestValid ? "合法" : "无效"}</Descriptions.Item>
        <Descriptions.Item label="schemaVersion">{report.schemaVersion ?? "-"}</Descriptions.Item>
        <Descriptions.Item label="productId">{report.productId ?? "-"}</Descriptions.Item>
        <Descriptions.Item label="pluginId">{report.pluginId ?? "-"}</Descriptions.Item>
        <Descriptions.Item label="version">{report.version ?? "-"}</Descriptions.Item>
        <Descriptions.Item label="classification">{report.classification ?? "legacy"}</Descriptions.Item>
        <Descriptions.Item label="runtimeKind">{report.runtimeKind ?? "-"}</Descriptions.Item>
        <Descriptions.Item label="交付方式">{report.deliveryMode ?? "普通插件"}</Descriptions.Item>
        <Descriptions.Item label="协议">{report.protocol ?? "-"}</Descriptions.Item>
        <Descriptions.Item label="文件数">{report.fileCount}</Descriptions.Item>
        <Descriptions.Item label="SHA-256"><Text copyable>{report.sha256}</Text></Descriptions.Item>
        <Descriptions.Item label="签名状态">{report.signatureStatus}</Descriptions.Item>
        <Descriptions.Item label="兼容当前版本">{report.compatible ? "是" : "否"}</Descriptions.Item>
        <Descriptions.Item label="Feature 入口" span={2}>{report.features.length > 0 ? report.features.join("、") : "-"}</Descriptions.Item>
        <Descriptions.Item label="Enhancement hooks" span={2}>{report.enhancementHooks.length > 0 ? report.enhancementHooks.join("、") : "-"}</Descriptions.Item>
        <Descriptions.Item label="适用场景" span={2}>{report.supportedScenes.length > 0 ? report.supportedScenes.join("、") : "-"}</Descriptions.Item>
        <Descriptions.Item label="权限" span={2}>{report.permissions.length > 0 ? report.permissions.join("、") : "无"}</Descriptions.Item>
        <Descriptions.Item label="凭据要求" span={2}>
          {report.credentialRequirements.length > 0
            ? report.credentialRequirements.map((item) => `${item.label ?? item.id}${item.provider ? `（${item.provider}）` : ""}`).join("、")
            : "无"}
        </Descriptions.Item>
      </Descriptions>
      <Divider />
      <Space wrap>
        {report.hasExecutables && <Tag color="red">包含可执行文件</Tag>}
        {report.hasScripts && <Tag color="red">包含脚本</Tag>}
        {report.hasSuspectedSecrets && <Tag color="red">疑似密钥</Tag>}
        {report.hasExternalUrls && <Tag color="orange">外部 URL</Tag>}
        {report.hasAbsolutePaths && <Tag color="orange">绝对路径</Tag>}
        {report.hasHighRiskPermissions && <Tag color="orange">高风险权限</Tag>}
      </Space>
      {report.errors.length > 0 && <Alert className="mt-3" type="error" message="阻止提交" description={report.errors.join("；")} />}
      {report.warnings.length > 0 && <Alert className="mt-3" type="warning" message="需人工复核" description={report.warnings.join("；")} />}
      {report.findings.length > 0 && (
        <Table
          className="mt-3"
          size="small"
          rowKey={(row, idx) => `${row.file}-${idx}`}
          dataSource={report.findings}
          pagination={false}
          columns={[
            { title: "级别", dataIndex: "severity" },
            { title: "类别", dataIndex: "category" },
            { title: "文件", dataIndex: "file" },
            { title: "说明", dataIndex: "message" },
          ]}
        />
      )}
      <Paragraph type="secondary" className="mt-3 mb-0">
        未接入复杂病毒扫描时不会显示为“安全”；本报告只代表本地静态检查。
      </Paragraph>
    </Card>
  );
}

export default function DeveloperCenterPage() {
  const renderCountRef = useRef(0);
  const initialLoadStartedRef = useRef(false);
  const loadEffectRunsRef = useRef(0);
  renderCountRef.current += 1;
  const [session, setSession] = useState<MarketplaceMockSession | null>(null);
  const [products, setProducts] = useState<DeveloperProduct[]>([]);
  const [dashboard, setDashboard] = useState<DeveloperDashboard | null>(null);
  const [selected, setSelected] = useState<DeveloperProduct | null>(null);
  const [report, setReport] = useState<MarketplacePackageReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [uploadOpen, setUploadOpen] = useState(false);
  const [uploadVersion, setUploadVersion] = useState("1.0.0");
  const [activeTab, setActiveTab] = useState("products");
  const [visitedTabs, setVisitedTabs] = useState<Set<string>>(() => new Set(["products"]));
  const [form] = Form.useForm<DeveloperProductFormValues>();
  const deliveryMode = Form.useWatch("deliveryMode", form);

  async function timedInvoke<T>(
    name: string,
    action: () => Promise<T>,
    records: Array<{ name: string; ms: number }>,
  ): Promise<T> {
    const startedAt = performance.now();
    try {
      return await action();
    } finally {
      records.push({ name, ms: Math.round(performance.now() - startedAt) });
    }
  }

  async function load(reason = "manual") {
    const startedAt = performance.now();
    const invokes: Array<{ name: string; ms: number }> = [];
    const s = await timedInvoke("marketplace_get_mock_session", marketplaceApi.getMockSession, invokes);
    setSession(s);
    let nextProducts: DeveloperProduct[] = [];
    let nextDashboard: DeveloperDashboard | null = null;
    if (s.canSell) {
      const [list, dash] = await Promise.all([
        timedInvoke("developer_list_products", developerApi.listProducts, invokes),
        timedInvoke("developer_get_dashboard", developerApi.getDashboard, invokes),
      ]);
      nextProducts = list;
      nextDashboard = dash;
      setProducts(nextProducts);
      setDashboard(nextDashboard);
      setSelected((prev) => nextProducts.find((p) => p.id === prev?.id) ?? nextProducts[0] ?? null);
    } else {
      setProducts([]);
      setDashboard(null);
      setSelected(null);
    }
    if (import.meta.env.DEV) {
      console.info("[DeveloperCenter] load", {
        reason,
        totalMs: Math.round(performance.now() - startedAt),
        renderCount: renderCountRef.current,
        effectRuns: loadEffectRunsRef.current,
        invokeCount: invokes.length,
        invokes,
        productCount: nextProducts.length,
        versionCount: nextProducts.reduce((sum, product) => sum + product.versions.length, 0),
        dashboardLoaded: Boolean(nextDashboard),
      });
    }
  }

  useEffect(() => {
    loadEffectRunsRef.current += 1;
    if (initialLoadStartedRef.current) {
      if (import.meta.env.DEV) {
        console.info("[DeveloperCenter] skipped duplicate StrictMode load", {
          effectRuns: loadEffectRunsRef.current,
          renderCount: renderCountRef.current,
        });
      }
      return;
    }
    initialLoadStartedRef.current = true;
    load("mount").catch((e) => message.error(String(e)));
  }, []);

  const latest = useMemo<DeveloperProductVersion | null>(() => {
    const versions = selected?.versions ?? [];
    return versions.length > 0 ? versions[versions.length - 1] : null;
  }, [selected]);

  function importWorkflowSchemaIntoForm() {
    const text = String(form.getFieldValue("workflowSchemaImport") || "");
    const fields = importWorkflowFieldsFromText(text);
    if (fields.length === 0) {
      message.warning("未能从粘贴内容中识别开始节点字段，请手动添加并核对星辰编排页字段名。");
      return;
    }
    const inputParameter =
      fields.find((field) => field.type === "multiline" || field.type === "string")?.key ??
      fields[0]?.key ??
      DEFAULT_WORKFLOW_INPUT_KEY;
    form.setFieldsValue({ workflowInputFields: fields as any, inputParameter });
    message.success(`已导入 ${fields.length} 个开始节点字段`);
  }

  async function createProduct(values: DeveloperProductFormValues) {
    setLoading(true);
    try {
      const option = productTypeOptions.find((o) => o.value === values.productType);
      const serviceConfiguration = buildServiceConfiguration(values);
      const product = await developerApi.createProduct({
        ...values,
        runtimeKind: option?.runtime ?? values.runtimeKind,
        tags: typeof (values.tags as unknown) === "string"
          ? String(values.tags).split(",").map((v) => v.trim()).filter(Boolean)
          : values.tags ?? [],
        priceAmount: values.licenseType === "free" ? 0 : values.priceAmount ?? 0,
        byokRequired: values.deliveryMode === "byok",
        protocol: values.deliveryMode === "byok" ? "xingchen-workflow-v1" : values.deliveryMode || null,
        serviceConfiguration,
      });
      const version = await developerApi.createVersion({
        productId: product.id,
        version: values.initialVersion || "1.0.0",
        changelog: "初始 AI 服务交付配置",
      });
      message.success("商品与初始 Manifest 草稿已创建");
      form.resetFields();
      await load();
      setSelected({ ...product, currentVersion: version.version, versions: [version] });
    } finally {
      setLoading(false);
    }
  }

  async function uploadPackage() {
    if (!selected) return;
    const picked = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: "Pomegranate 插件包", extensions: ["zip", "firstwork-plugin"] }],
    });
    if (!picked || Array.isArray(picked)) return;
    setLoading(true);
    try {
      const result = await developerApi.uploadPackage({
        productId: selected.id,
        version: uploadVersion,
        zipPath: picked,
        changelog: "本地开发者上传插件版本",
      });
      setReport(result);
      if (result.ok) {
        message.success(`${result.packageFormat === "v3-firstwork-plugin" ? "Manifest v3" : "Manifest v2"} 预检通过，包已保存到受控审核目录`);
      } else {
        message.error(result.errors?.[0] || "上传检查失败，未创建可提交版本");
      }
      await load();
      setUploadOpen(false);
    } finally {
      setLoading(false);
    }
  }

  async function submitSelected() {
    if (!selected || !latest) return;
    setLoading(true);
    try {
      const result = await developerApi.submitProduct({ productId: selected.id, version: latest.version });
      message.success(result.message);
      await load();
    } finally {
      setLoading(false);
    }
  }

  function handleTabChange(key: string) {
    setActiveTab(key);
    setVisitedTabs((prev) => {
      if (prev.has(key)) return prev;
      const next = new Set(prev);
      next.add(key);
      return next;
    });
  }

  if (session && !session.canSell) {
    return (
      <div className="commerce-page">
        <div className="commerce-page__inner">
        <RoleSwitcher session={session} onChanged={() => { void load("role-switch"); }} />
        <CommerceHero
          eyebrow="CREATOR ONBOARDING / LOCAL DEMO"
          title="把你的能力变成可安装商品"
          description="同一个个人账号既可以买，也可以申请成为创作者。申请后可创建插件、智能体、Hosted API 与 MCP 商品。"
          icon={<PackagePlus size={27} />}
          badge={<Tag color="gold">尚未开通创作者能力</Tag>}
          actions={(
            <Button type="primary" onClick={async () => { await marketplaceApi.applyDeveloper(); await load("apply-developer"); }}>
              申请成为本地演示创作者
            </Button>
          )}
          metrics={[
            { label: "账号", value: session.displayName, hint: "买家与创作者共用身份", tone: "teal" },
            { label: "交易", value: "未接入", hint: "不托管真实支付/提现", tone: "amber" },
            { label: "密钥", value: "不进入商品包", hint: "凭据由买家自行配置", tone: "blue" },
            { label: "审核", value: "本地流程", hint: "静态扫描 + 人工决策", tone: "coral" },
          ]}
        />
        <Alert className="mt-4" type="info" showIcon message="权限仍由 Rust 后端校验" description="未获得创作者身份前，前端按钮或参数都不能绕过后端创建和提交商品。" />
        </div>
      </div>
    );
  }

  return (
    <div className="commerce-page">
      <div className="commerce-page__inner">
      <RoleSwitcher session={session} onChanged={() => { void load("role-switch"); }} />
      <CommerceHero
        eyebrow="APP PUBLISHING / LOCAL DEMO"
        title="上传与发布"
        description="在 AI 应用市场内创建插件、Workflow/API/MCP 调用项并上传安装包。Pomegranate 只保存展示信息、Schema 与安全连接配置；星辰工作流本体、授权和实际运行仍在讯飞星辰平台完成。"
        icon={<Boxes size={27} />}
        badge={<Tag color="cyan">{session?.displayName ?? "本地用户"}</Tag>}
        actions={(
          <Button type="primary" icon={<PackagePlus size={16} />} onClick={() => handleTabChange("create")}>
            创建新商品
          </Button>
        )}
        metrics={[
          { label: "我的调用项", value: dashboard?.productCount ?? products.length, hint: "含草稿和已发布", tone: "teal" },
          { label: "审核流转中", value: products.filter((item) => ["submitted", "under_review"].includes(item.status)).length, hint: "等待平台处理", tone: "amber" },
          { label: "已配置服务", value: dashboard?.externalServiceCount ?? 0, hint: "来自 AI 资源中心绑定", tone: "blue" },
          { label: "调用次数", value: dashboard?.invocationCount ?? 0, hint: `成功 ${dashboard?.invocationSuccessCount ?? 0} / 异常 ${dashboard?.invocationFailedCount ?? 0}`, tone: "coral" },
        ]}
      />

      <div className="commerce-workflow">
        {[
          ["1", "定义调用项", "能力、输入输出 Schema 与授权说明"],
          ["2", "连接测试", "使用开发者自有凭据验证配置"],
          ["3", "提交审核", "安全证据与风险复核"],
          ["4", "用户绑定", "外部授权后在本机配置调用"],
        ].map(([number, title, detail]) => (
          <div className="commerce-workflow__step" key={number}>
            <span className="commerce-workflow__number">{number}</span>
            <strong>{title}</strong>
            {detail}
          </div>
        ))}
      </div>

      <Tabs
        className="commerce-panel p-4"
        activeKey={activeTab}
        onChange={handleTabChange}
        items={[
          {
            key: "products",
            label: "我的工作流商品",
            children: visitedTabs.has("products") ? (
              <div className="commerce-master-detail">
                  <Card className="commerce-table-card" title="工作流与能力调用项" extra={<Text type="secondary">{products.length} 个</Text>}>
                    <Table
                      size="small"
                      rowKey="id"
                      dataSource={products}
                      pagination={products.length > 8 ? { pageSize: 8, showSizeChanger: false } : false}
                      onRow={(record) => ({ onClick: () => setSelected(record) })}
                      rowClassName={(record) => record.id === selected?.id ? "ant-table-row-selected" : ""}
                      columns={[
                        { title: "名称", dataIndex: "name" },
                        { title: "状态", render: (_, row) => statusTag(row.status) },
                        { title: "授权", render: (_, row) => developerPriceText(row) },
                      ]}
                    />
                  </Card>
                  <Card
                    className="commerce-detail-card"
                    title={selected ? selected.name : "调用项详情"}
                    extra={<Button disabled={!selected} onClick={() => { setUploadVersion(latest?.version || selected?.currentVersion || "1.0.0"); setUploadOpen(true); }} loading={loading}>上传插件版本</Button>}
                  >
                    {selected ? (
                      <>
                        <Space wrap className="mb-4">
                          {statusTag(selected.status)}
                          <DeliveryModeTag mode={selected.deliveryMode} />
                          {selected.byokRequired && <Tag color="blue">BYOK</Tag>}
                          <Tag>{selected.productType}</Tag>
                        </Space>
                        <Descriptions column={2} size="small">
                          <Descriptions.Item label="商品 ID"><Text copyable>{selected.id}</Text></Descriptions.Item>
                          <Descriptions.Item label="状态">{statusTag(selected.status)}</Descriptions.Item>
                          <Descriptions.Item label="类型">{selected.productType}</Descriptions.Item>
                          <Descriptions.Item label="runtime">{selected.runtimeKind}</Descriptions.Item>
                          <Descriptions.Item label="BYOK">{selected.byokRequired ? "是" : "否"}</Descriptions.Item>
                          <Descriptions.Item label="交付方式"><DeliveryModeTag mode={selected.deliveryMode} /></Descriptions.Item>
                          <Descriptions.Item label="协议">{selected.protocol ?? "-"}</Descriptions.Item>
                          <Descriptions.Item label={selected.deliveryMode ? "外部参考价" : "本地演示价"}>
                            {developerPriceText(selected)}
                          </Descriptions.Item>
                        </Descriptions>
                        <Divider />
                        <Title level={5}>版本</Title>
                        <Table
                          size="small"
                          rowKey="id"
                          dataSource={selected.versions}
                          pagination={false}
                          columns={[
                            { title: "版本", dataIndex: "version" },
                            { title: "格式", render: (_, row) => <Tag color={row.packageFormat === "v3-firstwork-plugin" ? "blue" : "default"}>{row.packageFormat}</Tag> },
                            { title: "分类", render: (_, row) => row.classification ?? "legacy" },
                            { title: "状态", render: (_, row) => statusTag(row.status) },
                            { title: "扫描", render: (_, row) => <CommerceStatusTag status={row.scanStatus} /> },
                            { title: "锁定", render: (_, row) => row.packageLocked ? <Tag color="green">哈希已锁定</Tag> : <Tag>未锁定</Tag> },
                            { title: "Hash", dataIndex: "contentHash", ellipsis: true },
                          ]}
                        />
                        <Space className="mt-3">
                          <Button
                            type="primary"
                            icon={<ShieldCheck size={15} />}
                            disabled={!latest || latest.scanStatus === "failed"}
                            onClick={submitSelected}
                            loading={loading}
                          >
                            提交审核
                          </Button>
                        </Space>
                        <Divider />
                        <ReportView report={report} />
                      </>
                    ) : <Empty description="暂无商品" />}
                  </Card>
              </div>
            ) : null,
          },
          {
            key: "create",
            label: "创建调用项",
            children: visitedTabs.has("create") ? (
              <Card className="commerce-detail-card" title="创建工作流/API/MCP 调用项草稿" extra={<Tag color="gold">先定义，再上传版本包</Tag>}>
                <Alert
                  className="mb-4"
                  type="info"
                  showIcon
                  message="先选择交付方式"
                  description="Pomegranate 只保存调用项元数据、输入输出 Schema 与安全连接配置；凭据明文和星辰工作流本体永远不进入商品包，也不在本地伪造真实交易或结算。"
                />
                <Form
                  layout="vertical"
                  form={form}
                  onFinish={createProduct}
                  initialValues={{
                    licenseType: "free",
                    productType: "local-plugin",
                    byokRequired: false,
                    fileUploadRequired: false,
                    priceAmount: 0,
                    inputParameter: "AGENT_USER_INPUT",
                    workflowInputFields: [{
                      key: "AGENT_USER_INPUT",
                      label: "用户输入",
                      type: "multiline",
                      required: true,
                      placeholder: "请输入工作流开始节点文本",
                    }],
                    flowIdProvidedByUser: true,
                    requestMethod: "POST",
                    authenticationType: "bearer",
                    streaming: true,
                    transport: "streamable-http",
                    capabilities: "tools",
                    timeoutMs: 30000,
                    initialVersion: "1.0.0",
                  }}
                  onValuesChange={(changed) => {
                    if (changed.deliveryMode === "byok") {
                      form.setFieldsValue({
                        productType: "xingchen-workflow",
                        byokRequired: true,
                        inputParameter: "AGENT_USER_INPUT",
                        workflowInputFields: [{
                          key: "AGENT_USER_INPUT",
                          label: "用户输入",
                          type: "multiline",
                          required: true,
                          placeholder: "请输入工作流开始节点文本",
                        }],
                      });
                    } else if (changed.deliveryMode === "hosted-api") {
                      form.setFieldsValue({ productType: "xingchen-agent", byokRequired: false });
                    } else if (changed.deliveryMode === "remote-mcp") {
                      form.setFieldsValue({ productType: "mcp-connector", byokRequired: false });
                    }
                  }}
                >
                  <Row gutter={16} className="commerce-form-grid">
                    <Col span={12}><Form.Item name="name" label="名称" rules={[{ required: true }]}><Input /></Form.Item></Col>
                    <Col span={8}><Form.Item name="productType" label="商品类型" rules={[{ required: true }]}><Select options={productTypeOptions.map(({ value, label }) => ({ value, label }))} /></Form.Item></Col>
                    <Col span={4}><Form.Item name="initialVersion" label="初始版本" rules={[{ required: true, pattern: /^\d+\.\d+\.\d+$/ }]}><Input /></Form.Item></Col>
                    <Col span={24}><Form.Item name="description" label="简介" rules={[{ required: true }]}><Input /></Form.Item></Col>
                    <Col span={24}><Form.Item name="fullDescription" label="完整描述"><Input.TextArea rows={3} /></Form.Item></Col>
                    <Col span={8}><Form.Item name="category" label="分类"><Input /></Form.Item></Col>
                    <Col span={8}><Form.Item name="tags" label="标签，逗号分隔"><Input /></Form.Item></Col>
                    <Col span={8}><Form.Item name="licenseType" label="外部授权展示类型"><Select options={[{ value: "free", label: "免费/无需外部付费" }, { value: "one_time", label: "外部一次性授权" }, { value: "subscription", label: "外部订阅授权" }]} /></Form.Item></Col>
                    <Col span={8}><Form.Item name="priceAmount" label="外部参考价（分，可选展示）"><InputNumber min={0} className="w-full" /></Form.Item></Col>
                    <Col span={8}><Form.Item name="byokRequired" label="是否 BYOK"><Select options={[{ value: false, label: "否" }, { value: true, label: "是" }]} /></Form.Item></Col>
                    <Col span={8}><Form.Item name="fileUploadRequired" label="需要文件/图片上传"><Select options={[{ value: false, label: "否" }, { value: true, label: "是" }]} /></Form.Item></Col>
                    <Col span={24}><Form.Item name="dataDestination" label="数据发送说明"><Input.TextArea rows={2} /></Form.Item></Col>
                    <Col span={24}><Form.Item name="privacyNotice" label="隐私说明"><Input.TextArea rows={2} /></Form.Item></Col>
                    <Col span={24}><Form.Item name="usageGuide" label="使用说明"><Input.TextArea rows={2} /></Form.Item></Col>
                    <Col span={24}>
                      <Divider titlePlacement="start">AI 服务交付</Divider>
                    </Col>
                    <Col span={12}>
                      <Form.Item name="deliveryMode" label="交付方式">
                        <Select allowClear placeholder="普通插件无需选择" options={deliveryModeOptions} />
                      </Form.Item>
                    </Col>
                    {deliveryMode && (
                      <Col span={12}>
                        <Form.Item name="externalUrl" label="外部授权/发布页面 URL（可选）">
                          <Input placeholder="https://xingchen.xfyun.cn/... 或开发者授权说明页" />
                        </Form.Item>
                      </Col>
                    )}
                    {deliveryMode === "byok" && (
                      <>
                        <Col span={12}><Form.Item label="协议"><Input disabled value="xingchen-workflow-v1" /></Form.Item></Col>
                        <Col span={24}><Alert type="warning" showIcon message="不得填写或上传 API Key、API Secret" description="商品只声明 credentialId 引用和配置结构，买家凭据由 AI 资源中心安全保存。" /></Col>
                        <Col span={16}><Form.Item label="官方 Endpoint"><Input disabled value="https://xingchen-api.xf-yun.com/workflow/v1/chat/completions" /></Form.Item></Col>
                        <Col span={8}><Form.Item name="inputParameter" label="开始节点参数名" rules={[{ required: true }]}><Input /></Form.Item></Col>
                        <Col span={12}><Form.Item name="flowIdProvidedByUser" label="Flow ID 由用户填写"><Select options={[{ value: true, label: "是" }, { value: false, label: "否" }]} /></Form.Item></Col>
                        <Col span={24}>
                          <Form.Item
                            name="workflowSchemaImport"
                            label="导入字段 JSON Schema / 字段 JSON / YAML（可选）"
                            extra="官方未提供按 flow_id 自动发现 schema 的公开接口；请从星辰编排页核对字段名后粘贴或手动配置。"
                          >
                            <Input.TextArea rows={3} placeholder='例如 {"properties":{"major":{"type":"string"},"learning_days":{"type":"integer"}},"required":["major"]}' />
                          </Form.Item>
                          <Button className="mb-3" onClick={importWorkflowSchemaIntoForm}>
                            导入开始节点字段
                          </Button>
                          <Form.List name="workflowInputFields">
                            {(fields, { add, remove }) => (
                              <Form.Item
                                label="开始节点输入字段"
                                extra="字段名 key 会原样写入 parameters。空的非必填字段不会发送，文件字段会先由 Rust 后端上传后再传 URL。"
                              >
                                <Space direction="vertical" className="w-full" size={8}>
                                  {fields.map((field, index) => (
                                    <Card key={field.key} size="small" className="w-full">
                                      <Row gutter={8}>
                                        <Col span={6}>
                                          <Form.Item {...field} name={[field.name, "key"]} label="字段名 key" rules={[{ required: true }]}>
                                            <Input placeholder="major / AGENT_USER_INPUT" />
                                          </Form.Item>
                                        </Col>
                                        <Col span={5}>
                                          <Form.Item {...field} name={[field.name, "label"]} label="显示名称">
                                            <Input placeholder="专业" />
                                          </Form.Item>
                                        </Col>
                                        <Col span={5}>
                                          <Form.Item {...field} name={[field.name, "type"]} label="类型" rules={[{ required: true }]}>
                                            <Select options={WORKFLOW_FIELD_TYPE_OPTIONS} />
                                          </Form.Item>
                                        </Col>
                                        <Col span={4}>
                                          <Form.Item {...field} name={[field.name, "required"]} label="必填" valuePropName="checked">
                                            <Checkbox />
                                          </Form.Item>
                                        </Col>
                                        <Col span={4}>
                                          <Button disabled={fields.length <= 1} danger onClick={() => remove(field.name)}>
                                            删除
                                          </Button>
                                        </Col>
                                        <Col span={8}>
                                          <Form.Item {...field} name={[field.name, "defaultValue"]} label="默认值">
                                            <Input />
                                          </Form.Item>
                                        </Col>
                                        <Col span={8}>
                                          <Form.Item {...field} name={[field.name, "placeholder"]} label="占位提示">
                                            <Input />
                                          </Form.Item>
                                        </Col>
                                        <Col span={8}>
                                          <Form.Item {...field} name={[field.name, "options"]} label="select选项（逗号分隔）">
                                            <Input placeholder="A,B,C" />
                                          </Form.Item>
                                        </Col>
                                        <Col span={8}>
                                          <Form.Item {...field} name={[field.name, "fileConfig", "allowedExtensions"]} label="文件扩展名（逗号分隔）">
                                            <Input placeholder="pdf,docx,txt" />
                                          </Form.Item>
                                        </Col>
                                        <Col span={8}>
                                          <Form.Item {...field} name={[field.name, "fileConfig", "maxSizeMb"]} label="文件上限MB">
                                            <InputNumber min={1} max={200} className="w-full" />
                                          </Form.Item>
                                        </Col>
                                        <Col span={24}>
                                          <Form.Item {...field} name={[field.name, "description"]} label="说明">
                                            <Input />
                                          </Form.Item>
                                        </Col>
                                      </Row>
                                      <Text type="secondary">顺序：{index + 1}</Text>
                                    </Card>
                                  ))}
                                  <Button
                                    type="dashed"
                                    onClick={() => add({ key: "", label: "", type: "string", required: true })}
                                  >
                                    添加字段
                                  </Button>
                                </Space>
                              </Form.Item>
                            )}
                          </Form.List>
                        </Col>
                      </>
                    )}
                    {deliveryMode === "hosted-api" && (
                      <>
                        <Col span={24}><Alert type="info" showIcon message="Hosted API 当前只保留安全结构和 Mock 验证" description="真实托管服务必须使用 HTTPS，开发者的星辰源密钥只能保存在开发者服务端，不能随商品或客户端配置分发。" /></Col>
                        <Col span={16}><Form.Item name="endpoint" label="HTTPS Endpoint" rules={[{ required: true }]}><Input placeholder="https://api.example.com/v1/chat 或 mock://hosted-api" /></Form.Item></Col>
                        <Col span={8}><Form.Item name="requestMethod" label="请求方法"><Select options={[{ value: "POST" }, { value: "PUT" }]} /></Form.Item></Col>
                        <Col span={12}><Form.Item name="authenticationType" label="认证方式"><Select options={[{ value: "none" }, { value: "bearer" }]} /></Form.Item></Col>
                        <Col span={12}><Form.Item name="responseTextField" label="响应文本字段"><Input placeholder="data.text" /></Form.Item></Col>
                        <Col span={24}><Form.Item name="requestBodySchema" label="请求体 Schema"><Input.TextArea rows={3} /></Form.Item></Col>
                        <Col span={8}><Form.Item name="streaming" label="支持流式"><Select options={[{ value: true, label: "是" }, { value: false, label: "否" }]} /></Form.Item></Col>
                      </>
                    )}
                    {deliveryMode === "remote-mcp" && (
                      <>
                        <Col span={24}><Alert type="info" showIcon message="远程 MCP 本阶段只做安全 Mock 连通性和受控注册声明；不会启动脚本或系统命令。" /></Col>
                        <Col span={16}><Form.Item name="serverUrl" label="MCP Server URL" rules={[{ required: true }]}><Input placeholder="https://mcp.example.com 或 mock://remote-mcp" /></Form.Item></Col>
                        <Col span={8}><Form.Item name="transport" label="Transport"><Select options={[{ value: "streamable-http" }, { value: "sse" }]} /></Form.Item></Col>
                        <Col span={8}><Form.Item name="authenticationType" label="鉴权"><Select options={[{ value: "none" }, { value: "bearer" }]} /></Form.Item></Col>
                        <Col span={8}><Form.Item name="capabilities" label="Capabilities"><Input placeholder="tools,resources" /></Form.Item></Col>
                        <Col span={8}><Form.Item name="timeoutMs" label="超时（ms）"><InputNumber min={1000} max={120000} className="w-full" /></Form.Item></Col>
                      </>
                    )}
                  </Row>
                  <Button type="primary" htmlType="submit" loading={loading}>创建草稿</Button>
                </Form>
              </Card>
            ) : null,
          },
          {
            key: "dashboard",
            label: "调用统计",
            children: visitedTabs.has("dashboard") ? (
              <Card className="commerce-detail-card" title="调用项统计" extra={<ChartNoAxesCombined size={18} />}>
                {dashboard ? (
                  <>
                    <Row gutter={16}>
                      <Col span={6}><Statistic title="工作流/插件调用项" value={dashboard.productCount} /></Col>
                      <Col span={6}><Statistic title="已绑定服务配置" value={dashboard.externalServiceCount} /></Col>
                      <Col span={6}><Statistic title="调用次数" value={dashboard.invocationCount} /></Col>
                      <Col span={6}><Statistic title="成功 / 异常" value={`${dashboard.invocationSuccessCount} / ${dashboard.invocationFailedCount}`} /></Col>
                      <Col span={6}><Statistic title="本地安装数" value={dashboard.mockInstallCount} /></Col>
                      <Col span={6}><Statistic title="本地启用数" value={dashboard.mockEnabledCount} /></Col>
                    </Row>
                    <Divider />
                    <Alert
                      type="warning"
                      showIcon
                      message="Legacy 本地模拟交易数据，仅用于历史演示"
                      description="当前未接入讯飞官方交易/结算 API，也不实现 Pomegranate 资金托管、余额、提现或自动分账。下方金额不代表真实收入。"
                    />
                    <Row gutter={16} className="mt-4">
                      <Col span={6}><Statistic title="历史模拟订单" value={dashboard.mockOrderCount} /></Col>
                      <Col span={6}><Statistic title="历史模拟获取" value={dashboard.mockAcquireCount} /></Col>
                      <Col span={6}><Statistic title="历史模拟流水" value={cents(dashboard.grossAmount)} /></Col>
                      <Col span={6}><Statistic title="历史模拟收益" value={cents(dashboard.developerAmount)} /></Col>
                    </Row>
                  </>
                ) : <Empty />}
              </Card>
            ) : null,
          },
        ]}
      />
      <Modal
        title="上传商品版本包"
        open={uploadOpen}
        confirmLoading={loading}
        onCancel={() => setUploadOpen(false)}
        onOk={uploadPackage}
        okText="选择插件包并上传"
      >
        <Paragraph>
          支持 <Text code>.firstwork-plugin</Text>（Manifest v3）和 <Text code>.zip</Text>（Manifest v2 兼容）。
          v3 包会进入插件平台严格预检，不会改名或送入旧解析器；审核批准后对应文件与 SHA-256 将被锁定。
        </Paragraph>
        <Alert
          className="mb-3"
          type="warning"
          showIcon
          message="每个新版本必须单独上传和审核"
          description="manifest.version 必须与下方版本一致。已批准版本不能原地替换包内容；如需更新，请创建并上传新的语义化版本。"
        />
        <Input value={uploadVersion} onChange={(e) => setUploadVersion(e.target.value)} placeholder="1.0.0" />
      </Modal>
      </div>
    </div>
  );
}
