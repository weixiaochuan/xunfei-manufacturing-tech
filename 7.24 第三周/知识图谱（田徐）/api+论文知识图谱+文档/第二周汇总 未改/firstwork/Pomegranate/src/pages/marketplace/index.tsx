import { useEffect, useMemo, useState, type CSSProperties } from "react";
import {
  Alert,
  Badge,
  Button,
  Card,
  Checkbox,
  Descriptions,
  Divider,
  Drawer,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
  message,
} from "antd";
import { useNavigate } from "react-router-dom";
import {
  Bot,
  CheckCircle2,
  Download,
  PackageCheck,
  Plug,
  RotateCcw,
  Search,
  ShieldAlert,
  ShoppingBag,
  Sparkles,
  Store,
  Trash2,
} from "lucide-react";
import { credentialApi, externalAgentApi, marketplaceApi } from "@/lib/api";
import {
  DEFAULT_WORKFLOW_INPUT_KEY,
  WORKFLOW_FIELD_TYPE_OPTIONS,
  buildWorkflowRequestMapping,
  normalizeWorkflowInputFields,
  workflowFieldsFromConfigurationSchema,
} from "@/lib/workflowSchema";
import { notifyDeclarativePluginToolbarChanged } from "@/services/declarativePluginEvents";
import {
  CommerceHero,
  CommerceStatusTag,
  DeliveryModeTag,
} from "@/components/marketplace/CommerceShell";
import type {
  MarketplaceActionResult,
  MarketplaceProductQuery,
  MarketplaceProductDetail,
  MarketplaceProductSummary,
  LocalAccountProfile,
  MarketplaceMockSession,
  MarketplaceOrder,
  MarketplaceReviewInfo,
  CredentialInfo,
  ProductType,
  WorkflowInputField,
} from "@/types";

const { Text, Title, Paragraph } = Typography;

type TabKey =
  | "discover"
  | "free"
  | "agent"
  | "workflow"
  | "mcp"
  | "prompt"
  | "acquired"
  | "installed";

const TAB_LABELS: Array<{ key: TabKey; label: string }> = [
  { key: "discover", label: "发现" },
  { key: "free", label: "免费" },
  { key: "agent", label: "智能体" },
  { key: "workflow", label: "工作流" },
  { key: "mcp", label: "MCP" },
  { key: "prompt", label: "Prompt/模板" },
  { key: "acquired", label: "已授权/已获取" },
  { key: "installed", label: "已安装" },
];

function priceText(item: MarketplaceProductSummary): string {
  if (item.deliveryMode) {
    if (item.price.amount === 0) return "外部授权或免费连接器，Pomegranate 不处理付款";
    return `外部参考价 ¥${(item.price.amount / 100).toFixed(2)}，需在讯飞星辰或开发者处完成授权`;
  }
  if (item.price.amount === 0) return "免费";
  return `¥${(item.price.amount / 100).toFixed(2)} 模拟价格，不会真实扣款`;
}

function priceFromCents(amount: number): string {
  return `CNY ${(amount / 100).toFixed(2)}`;
}

function compactPriceText(item: MarketplaceProductSummary): string {
  if (item.deliveryMode) return item.price.amount === 0 ? "外部授权" : "外部参考价";
  return item.price.amount === 0 ? "免费" : `¥${(item.price.amount / 100).toFixed(2)} · 模拟`;
}

function isAiServiceProduct(item: MarketplaceProductSummary | MarketplaceProductDetail): boolean {
  return Boolean(item.deliveryMode);
}

function externalAuthorizationUrl(detail: MarketplaceProductDetail): string {
  const schema = detail.configurationSchema as Record<string, unknown> | undefined;
  const candidates = [
    schema?.externalUrl,
    schema?.externalAuthorizationUrl,
    schema?.xingchenConsoleUrl,
  ];
  for (const candidate of candidates) {
    if (typeof candidate === "string" && candidate.trim()) return candidate.trim();
    if (candidate && typeof candidate === "object" && "default" in candidate) {
      const value = String((candidate as { default?: unknown }).default ?? "").trim();
      if (value) return value;
    }
  }
  return "";
}

function entitlementStatusText(detail: MarketplaceProductDetail): string {
  const status = detail.entitlement?.status;
  if (status === "external_authorized") return "已绑定外部授权";
  if (status === "active") return "本地授权有效";
  if (status === "expired") return "授权已过期";
  if (status === "revoked") return "授权已撤销";
  if (status === "unavailable") return "授权不可用";
  if (status === "unknown") return "授权未知";
  return "未绑定授权";
}

function productTypeLabel(type: ProductType): string {
  const map: Record<ProductType, string> = {
    "local-plugin": "本地插件",
    "declarative-ui": "声明式UI",
    "prompt-pack": "Prompt包",
    "xingchen-agent": "星辰智能体",
    "xingchen-workflow": "星辰工作流",
    "xingchen-mcp": "星辰MCP",
    "mcp-connector": "MCP连接器",
    "knowledge-template": "知识库模板",
    "database-template": "数据库模板",
    "file-image-agent": "文件/图片智能体",
    "ppt-master-extension": "PPT扩展",
    "learning-assistant-extension": "助学扩展",
  };
  return map[type] ?? type;
}

function iconFor(item: MarketplaceProductSummary) {
  if (item.runtimeKind === "prompt-pack") return <Sparkles size={22} />;
  if (item.runtimeKind === "mcp-connector") return <Plug size={22} />;
  if (item.runtimeKind === "xingchen-agent") return <Bot size={22} />;
  return <Store size={22} />;
}

function productAccent(item: MarketplaceProductSummary) {
  if (item.runtimeKind === "prompt-pack") return "#d38c28";
  if (item.runtimeKind === "mcp-connector") return "#3f6fa2";
  if (item.runtimeKind === "xingchen-agent" || item.runtimeKind === "xingchen-workflow") return "#0f766e";
  return "#d66749";
}

function useMarketplaceProducts(tab: TabKey, keyword: string, productType: ProductType | "all") {
  const [items, setItems] = useState<MarketplaceProductSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);

  const query = useMemo<MarketplaceProductQuery>(() => {
    const base: MarketplaceProductQuery = {
      keyword: keyword.trim() || null,
      productType: productType === "all" ? null : productType,
    };
    if (tab === "free") base.freeOnly = true;
    if (tab === "agent") base.productType = "xingchen-agent";
    if (tab === "workflow") base.productType = "xingchen-workflow";
    if (tab === "mcp") base.runtimeKind = "mcp-connector";
    if (tab === "prompt") base.productType = "prompt-pack";
    if (tab === "acquired") base.acquiredOnly = true;
    if (tab === "installed") base.installedOnly = true;
    return base;
  }, [keyword, productType, tab]);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setError(null);
    marketplaceApi
      .searchProducts(query)
      .then((data) => {
        if (alive) setItems(data);
      })
      .catch((e) => {
        if (alive) setError(String(e));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [query, tick]);

  return { items, loading, error, refresh: () => setTick((v) => v + 1) };
}

export default function MarketplacePage() {
  const navigate = useNavigate();
  const [tab, setTab] = useState<TabKey>("discover");
  const [keyword, setKeyword] = useState("");
  const [productType, setProductType] = useState<ProductType | "all">("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<MarketplaceProductDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [actionLoading, setActionLoading] = useState(false);
  const [setupOpen, setSetupOpen] = useState(false);
  const [ordersOpen, setOrdersOpen] = useState(false);
  const [accounts, setAccounts] = useState<LocalAccountProfile[]>([]);
  const [session, setSession] = useState<MarketplaceMockSession | null>(null);
  const [orders, setOrders] = useState<MarketplaceOrder[]>([]);
  const [reviews, setReviews] = useState<MarketplaceReviewInfo[]>([]);
  const [credentials, setCredentials] = useState<CredentialInfo[]>([]);
  const [setupForm] = Form.useForm();
  const [reviewForm] = Form.useForm();
  const setupCredentialMode = Form.useWatch("credentialMode", setupForm);
  const { items, loading, error, refresh } = useMarketplaceProducts(tab, keyword, productType);

  async function loadAccountState() {
    const [accountRows, currentSession, orderRows] = await Promise.all([
      marketplaceApi.listAccounts(),
      marketplaceApi.getMockSession(),
      marketplaceApi.listOrders(),
    ]);
    setAccounts(accountRows);
    setSession(currentSession);
    setOrders(orderRows);
  }

  useEffect(() => {
    loadAccountState().catch((e) => message.error(String(e)));
  }, []);

  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      return;
    }
    let alive = true;
    setDetailLoading(true);
    marketplaceApi
      .getProduct(selectedId)
      .then((data) => {
        if (alive) {
          setDetail(data);
          marketplaceApi
            .listReviews(data.id)
            .then((rows) => {
              if (alive) setReviews(rows);
            })
            .catch(() => {
              if (alive) setReviews([]);
            });
        }
      })
      .catch((e) => message.error(String(e)))
      .finally(() => {
        if (alive) setDetailLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [selectedId]);

  async function reloadDetail(productId = selectedId) {
    refresh();
    await loadAccountState().catch(() => undefined);
    if (!productId) return;
    const data = await marketplaceApi.getProduct(productId);
    setDetail(data);
    setReviews(await marketplaceApi.listReviews(productId));
  }

  async function switchAccount(userId: string) {
    setActionLoading(true);
    try {
      const next = await marketplaceApi.switchAccount(userId);
      setSession(next);
      await loadAccountState();
      refresh();
      if (selectedId) {
        await reloadDetail(selectedId);
      }
      message.success(`已切换到 ${next.displayName}`);
    } catch (e) {
      message.error(String(e));
    } finally {
      setActionLoading(false);
    }
  }

  async function runAction(action: () => Promise<MarketplaceActionResult>) {
    setActionLoading(true);
    try {
      const result = await action();
      if (result.requiresPermissionConfirmation && result.permissionDiff) {
        Modal.confirm({
          title: "确认插件权限",
          content: (
            <div>
              <Paragraph>{result.message}</Paragraph>
              <Space wrap>
                {result.permissionDiff.added.map((p) => (
                  <Tag color="blue" key={p}>{p}</Tag>
                ))}
              </Space>
            </div>
          ),
          okText: "确认",
          cancelText: "取消",
          onOk: async () => {
            if (!detail) return;
            setActionLoading(true);
            try {
              const confirmed = detail.installed
                ? await marketplaceApi.updateProduct({
                    productId: detail.id,
                    confirmAddedPermissions: true,
                  })
                : await marketplaceApi.installProduct({
                    productId: detail.id,
                    confirmPermissions: true,
                  });
              message.success(confirmed.message);
              notifyDeclarativePluginToolbarChanged();
              await reloadDetail(detail.id);
            } finally {
              setActionLoading(false);
            }
          },
          onCancel: async () => {
            if (!detail) return;
            try {
              await marketplaceApi.recordPermissionRejection({
                productId: detail.id,
                action: detail.installed ? "update" : "install",
              });
            } catch (e) {
              console.warn("[Marketplace] failed to record permission rejection:", e);
            }
          },
        });
      } else if (result.ok) {
        message.success(result.message);
        notifyDeclarativePluginToolbarChanged();
        await reloadDetail(result.productId);
      } else {
        message.warning(result.message);
      }
    } catch (e) {
      message.error(String(e));
    } finally {
      setActionLoading(false);
    }
  }

  async function mockTest(productId: string) {
    setActionLoading(true);
    try {
      const result = await marketplaceApi.mockTestProduct(productId);
      message.success(result.message);
    } catch (e) {
      message.error(String(e));
    } finally {
      setActionLoading(false);
    }
  }

  async function requestRefund(order: MarketplaceOrder) {
    Modal.confirm({
      title: "Request mock refund",
      content: "This revokes the entitlement and writes refund/reversal ledger entries. No real money is involved.",
      okText: "Confirm refund",
      cancelText: "Cancel",
      onOk: async () => {
        const result = await marketplaceApi.requestRefund({
          orderId: order.id,
          reason: "local demo refund",
        });
        message.success(result.message);
        await reloadDetail(order.productId);
      },
    });
  }

  async function submitReview() {
    if (!detail) return;
    const values = await reviewForm.validateFields();
    const review = await marketplaceApi.submitReview({
      productId: detail.id,
      orderId: Number(values.orderId),
      rating: Number(values.rating),
      content: String(values.content),
    });
    message.success("Review submitted");
    reviewForm.resetFields();
    setReviews((prev) => [review, ...prev.filter((item) => item.id !== review.id)]);
  }

  function configDefault(key: string): string {
    const schema = detail?.configurationSchema as Record<string, unknown> | undefined;
    const value = schema?.[key];
    if (value && typeof value === "object" && "default" in value) {
      return String((value as { default?: unknown }).default ?? "");
    }
    return "";
  }

  function buildSetupWorkflowRequestMapping(values: Record<string, any>) {
    const fields = normalizeWorkflowInputFields(
      (Array.isArray(values.workflowInputFields) ? values.workflowInputFields : []).map((field: WorkflowInputField, index: number) => ({
        ...field,
        key: String(field.key || "").trim(),
        label: String(field.label || field.key || "").trim(),
        order: index,
        required: field.required !== false,
      })),
      String(values.inputParameter || DEFAULT_WORKFLOW_INPUT_KEY),
    );
    const inputParameter =
      fields.find((field) => field.type === "multiline" || field.type === "string")?.key ??
      fields[0]?.key ??
      DEFAULT_WORKFLOW_INPUT_KEY;
    return buildWorkflowRequestMapping(fields, inputParameter);
  }

  async function openServiceSetup() {
    if (!detail?.deliveryMode) return;
    const rows = await credentialApi.list();
    const workflowFields = workflowFieldsFromConfigurationSchema(
      detail.configurationSchema,
      configDefault("inputParameter") || DEFAULT_WORKFLOW_INPUT_KEY,
    );
    setCredentials(rows);
    setupForm.resetFields();
    setupForm.setFieldsValue({
      credentialMode: "existing",
      endpoint: detail.deliveryMode === "remote-mcp" ? configDefault("serverUrl") : configDefault("endpoint"),
      inputParameter: workflowFields[0]?.key || configDefault("inputParameter") || DEFAULT_WORKFLOW_INPUT_KEY,
      workflowInputFields: workflowFields,
      responseTextField: configDefault("responseTextField") || "answer",
      networkPermissionConfirmed: false,
    });
    setSetupOpen(true);
  }

  async function createSetupCredential(values: Record<string, unknown>): Promise<string | null> {
    if (values.credentialMode !== "new") return String(values.credentialId || "") || null;
    const mode = detail?.deliveryMode;
    const created = await credentialApi.create({
      provider: mode === "byok" ? "xingchen" : mode || "hosted-api",
      credentialType: mode === "byok" ? "app_key_secret" : "bearer_token",
      label: String(values.credentialLabel || `${detail?.name ?? "AI 服务"}凭据`),
      ownerScope: "local-user",
      secrets: mode === "byok"
        ? {
            appId: String(values.appId || ""),
            apiKey: String(values.apiKey || ""),
            apiSecret: String(values.apiSecret || ""),
          }
        : { bearerToken: String(values.bearerToken || "") || null },
    });
    setupForm.setFieldsValue({ appId: undefined, apiKey: undefined, apiSecret: undefined, bearerToken: undefined });
    return created.id;
  }

  async function saveServiceSetup() {
    if (!detail?.deliveryMode) return;
    const values = await setupForm.validateFields();
    setActionLoading(true);
    try {
      const credentialId = await createSetupCredential(values);
      if (detail.deliveryMode === "byok") {
        if (!credentialId) throw new Error("请选择或新建讯飞星辰 Workflow 凭据");
        const agent = await externalAgentApi.create({
          productId: detail.id,
          name: String(values.agentName || detail.name),
          endpoint: "https://xingchen-api.xf-yun.com/workflow/v1/chat/completions",
          flowId: String(values.flowId || "").trim(),
          protocolType: "xingchen_workflow_v1",
          authenticationType: "bearer",
          credentialId,
          streamingType: "sse",
          requestMappingJson: buildSetupWorkflowRequestMapping(values),
          responseMappingJson: JSON.stringify({ textField: values.responseTextField || "answer" }),
          sessionMappingJson: "{}",
          errorMappingJson: "{}",
          mockMode: false,
          enabled: true,
        });
        const result = await externalAgentApi.testConnection(agent.id);
        if (!result.ok) {
          await externalAgentApi.delete(agent.id).catch(() => undefined);
          Modal.error({
            title: "讯飞 Workflow 连接测试失败",
            width: 680,
            content: (
              <Space direction="vertical" className="w-full">
                <Paragraph>{result.message}</Paragraph>
                {result.errorCode && <Text>错误码：{result.errorCode}</Text>}
                {result.httpStatus && <Text>HTTP status：{result.httpStatus}</Text>}
                {result.requestId && <Text copyable={{ text: result.requestId }}>请求 ID：{result.requestId}</Text>}
                <Alert type="warning" showIcon message="配置建议" description="检查 Key/Secret 是否填反、Flow ID、工作流是否发布，以及凭据与 Flow ID 是否属于同一星辰应用授权。" />
              </Space>
            ),
          });
          return;
        }
        message.success("真实连接测试通过，已创建 ExternalAgent");
      } else {
        const configured = await marketplaceApi.configureService({
          productId: detail.id,
          credentialId,
          networkPermissionConfirmed: Boolean(values.networkPermissionConfirmed),
        });
        const tested = await marketplaceApi.mockTestProduct(detail.id);
        message.success(`${configured.message}；${tested.message}`);
      }
      setSetupOpen(false);
      await reloadDetail(detail.id);
    } catch (error) {
      message.error(String(error));
    } finally {
      setActionLoading(false);
    }
  }

  function renderCard(item: MarketplaceProductSummary) {
    return (
      <Card
        key={item.id}
        hoverable
        onClick={() => setSelectedId(item.id)}
        className="market-product-card"
        style={{ "--product-accent": productAccent(item) } as CSSProperties}
        styles={{ body: { height: "100%" } }}
      >
        <div className="flex h-full flex-col gap-3">
          <div className="flex items-start gap-3">
            <div className="market-product-card__icon">
              {iconFor(item)}
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center justify-between gap-2">
                <Text strong ellipsis>{item.name}</Text>
                <CommerceStatusTag status={item.revoked ? "revoked" : item.status} />
              </div>
              <Text type="secondary" className="text-xs">{item.developerName}</Text>
            </div>
          </div>
          <Paragraph className="mb-0 text-sm" ellipsis={{ rows: 2 }}>
            {item.description}
          </Paragraph>
          <Space wrap size={[6, 6]}>
            <Tag>{productTypeLabel(item.productType)}</Tag>
            <DeliveryModeTag mode={item.deliveryMode} />
            <Tag color={item.price.amount === 0 ? "green" : "gold"}>{compactPriceText(item)}</Tag>
            {item.byokRequired && <Tag color="blue">BYOK</Tag>}
            {item.acquired && <Tag icon={<CheckCircle2 size={12} />} color="success">已获取</Tag>}
            {item.installed && <Tag icon={<PackageCheck size={12} />} color="processing">已安装</Tag>}
            {item.hasUpdate && <Tag color="purple">可更新 {item.updateVersion}</Tag>}
          </Space>
          <div className="mt-auto flex items-center justify-between pt-2">
            <Text type="secondary" className="text-xs">
              v{item.currentVersion} · {item.permissionSummary.length || item.permissions.length} 项权限
            </Text>
            <Button type="link" size="small">
              {item.installed ? "管理" : item.acquired ? "安装" : "查看详情"}
            </Button>
          </div>
        </div>
      </Card>
    );
  }

  return (
    <div className="commerce-page">
      <div className="commerce-page__inner">
        <CommerceHero
          eyebrow="FIRSTWORK / LOCAL MARKETPLACE"
          title="AI 应用市场"
          description="发现可以真正安装到工作流中的插件、Prompt、智能体与 MCP。每个商品都清楚展示权限、数据去向和交付方式。"
          icon={<ShoppingBag size={27} />}
          badge={<Badge color="gold" text="本地演示模式" />}
          actions={(
            <>
            <Select
              value={session?.userId}
              loading={actionLoading}
              onChange={switchAccount}
              style={{ width: 240 }}
              placeholder="选择本地演示账号"
              options={accounts.map((account) => ({
                value: account.userId,
                label: `${account.nickname}${account.canAdmin ? " / 管理员" : account.canSell ? " / 创作者" : " / 买家"}`,
              }))}
            />
            <Button onClick={() => setOrdersOpen(true)}>历史演示订单</Button>
            </>
          )}
          metrics={[
            { label: "当前结果", value: items.length, hint: "符合当前筛选", tone: "teal" },
            { label: "已授权/已获取", value: items.filter((item) => item.acquired).length, hint: "当前账号本地或外部授权", tone: "blue" },
            { label: "已安装", value: items.filter((item) => item.installed).length, hint: "进入本地运行时", tone: "amber" },
            { label: "待更新", value: items.filter((item) => item.hasUpdate).length, hint: "需重新确认变化", tone: "coral" },
          ]}
        />

        <div className="commerce-panel commerce-filterbar">
          <div className="commerce-segments">
            {TAB_LABELS.map((item) => (
              <Button
                key={item.key}
                className="commerce-segment"
                type={tab === item.key ? "primary" : "text"}
                onClick={() => setTab(item.key)}
              >
                {item.label}
              </Button>
            ))}
          </div>
          <Space wrap>
            <Input
              allowClear
              prefix={<Search size={16} />}
              placeholder="搜索商品、开发者或描述"
              value={keyword}
              onChange={(e) => setKeyword(e.target.value)}
              style={{ width: 260 }}
            />
            <Select
              value={productType}
              onChange={setProductType}
              style={{ width: 180 }}
              options={[
                { value: "all", label: "全部类型" },
                { value: "prompt-pack", label: "Prompt包" },
                { value: "xingchen-agent", label: "星辰智能体" },
                { value: "xingchen-workflow", label: "星辰工作流" },
                { value: "mcp-connector", label: "MCP连接器" },
                { value: "local-plugin", label: "本地插件" },
                { value: "declarative-ui", label: "声明式UI" },
              ]}
            />
          </Space>
        </div>

        <Alert
          className="mb-4"
          type="info"
          showIcon
          message="Pomegranate 不处理真实支付、余额、提现或分账；AI 服务商品需要前往讯飞星辰或开发者处完成授权后再在本机绑定。"
        />

        <div className="commerce-section-heading">
          <div>
            <h3>{TAB_LABELS.find((item) => item.key === tab)?.label ?? "发现"}</h3>
            <p>点击商品卡查看权限、授权状态、安装动作和数据去向。</p>
          </div>
          <Text type="secondary">{loading ? "正在刷新目录…" : `共 ${items.length} 个商品`}</Text>
        </div>

        {error && <Alert className="mb-4" type="error" showIcon message={error} />}
        {loading ? (
          <div className="flex min-h-64 items-center justify-center">
            <Spin />
          </div>
        ) : items.length === 0 ? (
          <Empty description="暂无匹配商品" />
        ) : (
          <div className="market-product-grid">
            {items.map(renderCard)}
          </div>
        )}
      </div>

      <Drawer
        open={!!selectedId}
        onClose={() => setSelectedId(null)}
        width={620}
        title={detail?.name ?? "商品详情"}
      >
        {detailLoading || !detail ? (
          <div className="flex min-h-64 items-center justify-center">
            <Spin />
          </div>
        ) : (
          <div className="space-y-4">
            <Alert
              type={detail.revoked ? "error" : "warning"}
              showIcon
              message={
                detail.revoked
                  ? "该商品或版本已吊销，不能新安装、更新或启用。"
                  : isAiServiceProduct(detail)
                    ? "AI 服务授权不在 Pomegranate 内完成；请在讯飞星辰平台或开发者处获得授权后绑定到本机。"
                    : "本地演示市场：未接真实支付，未接真实签名服务。"
              }
            />
            <div className="flex items-start gap-3">
              <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-indigo-50 text-indigo-600">
                {iconFor(detail)}
              </div>
              <div>
                <Title level={4} className="!mb-1">{detail.name}</Title>
                <Text type="secondary">{detail.developerName}</Text>
              </div>
            </div>
            <Paragraph>{detail.fullDescription}</Paragraph>
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label="版本">v{detail.currentVersion}</Descriptions.Item>
              <Descriptions.Item label="创作者">
                {detail.sellerNickname ?? detail.developerName} ({detail.sellerUserId ?? detail.developerId})
              </Descriptions.Item>
              <Descriptions.Item label="更新说明">{detail.changelog}</Descriptions.Item>
              <Descriptions.Item label="最低 firstwork 版本">{detail.minAppVersion ?? "未声明"}</Descriptions.Item>
              <Descriptions.Item label="商品类型">{productTypeLabel(detail.productType)}</Descriptions.Item>
              <Descriptions.Item label="运行时">{detail.runtimeKind}</Descriptions.Item>
              <Descriptions.Item label="交付方式"><DeliveryModeTag mode={detail.deliveryMode} /></Descriptions.Item>
              <Descriptions.Item label="服务协议">{detail.protocol ?? "未声明"}</Descriptions.Item>
              <Descriptions.Item label="价格">{priceText(detail)}</Descriptions.Item>
              <Descriptions.Item label="许可证">{detail.licenseType}</Descriptions.Item>
              <Descriptions.Item label="授权状态">{entitlementStatusText(detail)}</Descriptions.Item>
              {isAiServiceProduct(detail) && (
                <Descriptions.Item label="外部授权入口">
                  {externalAuthorizationUrl(detail) ? (
                    <Text copyable={{ text: externalAuthorizationUrl(detail) }}>
                      {externalAuthorizationUrl(detail)}
                    </Text>
                  ) : "未提供，请按使用说明联系开发者或前往星辰平台"}
                </Descriptions.Item>
              )}
              <Descriptions.Item label="签名状态">{detail.signatureStatus}</Descriptions.Item>
              <Descriptions.Item label="完整性">{detail.integrityStatus}</Descriptions.Item>
              <Descriptions.Item label="数据去向">{detail.dataDestination ?? "未声明"}</Descriptions.Item>
              <Descriptions.Item label="文件/图片">{detail.fileUploadNotice ?? "未声明"}</Descriptions.Item>
            </Descriptions>
            <div>
              <Text strong>权限</Text>
              <div className="mt-2">
                <Space wrap>
                  {detail.permissions.length === 0 ? (
                    <Tag>无权限</Tag>
                  ) : (
                    detail.permissions.map((p: string) => <Tag key={p}>{p}</Tag>)
                  )}
                </Space>
              </div>
            </div>
            {detail.configurationChanged && (
              <Alert type="warning" showIcon message="新版本包含配置变化" description="Endpoint、协议、交付方式或配置 Schema 已变化，更新前请重新确认数据去向和凭据引用。" />
            )}
            {detail.credentialRequirements?.length > 0 && (
              <Alert
                type="info"
                showIcon
                message="BYOK 凭据要求"
                description="用户需要在后续凭据配置页填写自己的讯飞星辰 APPID、API Key、API Secret 或 Token。前端只保存 credentialId，不读取密钥明文。"
              />
            )}
            {detail.deliveryMode === "byok" && (
              <Alert
                type="warning"
                showIcon
                message="BYOK 授权关系"
                description="API Key、API Secret 和 Flow ID 必须属于同一星辰应用授权。商品不包含开发者密钥，credentialId 只引用 AI 资源中心中的安全凭据。"
              />
            )}
            {detail.deliveryMode === "hosted-api" && (
              <Alert type="info" showIcon message="开发者托管 API" description={`数据将发送给 ${detail.developerName} 声明的服务；本阶段仅提供 Mock Provider，不代表真实托管服务已上线。`} />
            )}
            {detail.deliveryMode === "remote-mcp" && (
              <Alert type="info" showIcon message="远程 MCP" description="安装向导会展示远程 URL、工具能力和网络权限；本阶段仅保存受控 Mock 注册，不执行任何本地命令。" />
            )}
            {detail.riskNotes?.length > 0 && (
              <Alert
                type="warning"
                showIcon
                icon={<ShieldAlert size={16} />}
                message="风险提示"
                description={
                  <ul className="m-0 pl-4">
                    {detail.riskNotes.map((n: string) => <li key={n}>{n}</li>)}
                  </ul>
                }
              />
            )}
            <Divider />
            <Space wrap>
              {!detail.acquired && isAiServiceProduct(detail) && (
                <>
                  <Button
                    icon={<ShoppingBag size={16} />}
                    disabled={!externalAuthorizationUrl(detail)}
                    onClick={() => {
                      const url = externalAuthorizationUrl(detail);
                      if (url) window.open(url, "_blank", "noopener,noreferrer");
                    }}
                  >
                    前往星辰/开发者获取授权
                  </Button>
                  <Button
                    type="primary"
                    icon={<Download size={16} />}
                    loading={actionLoading}
                    disabled={detail.selfOwned || detail.revoked}
                    onClick={() =>
                      runAction(() =>
                        marketplaceApi.bindExternalAuthorization({
                          productId: detail.id,
                          note: "用户确认已在外部获得授权",
                        }),
                      )
                    }
                  >
                    我已获得外部授权，绑定到本机
                  </Button>
                </>
              )}
              {!detail.acquired && !isAiServiceProduct(detail) && detail.price.amount === 0 && (
                <Button
                  type="primary"
                  icon={<Download size={16} />}
                  loading={actionLoading}
                  disabled={detail.selfOwned}
                  onClick={() =>
                    runAction(() => marketplaceApi.acquireProduct({ productId: detail.id }))
                  }
                >
                  免费获取
                </Button>
              )}
              {!detail.acquired && !isAiServiceProduct(detail) && detail.price.amount > 0 && (
                <Button
                  type="primary"
                  icon={<ShoppingBag size={16} />}
                  loading={actionLoading}
                  disabled={detail.selfOwned}
                  onClick={() =>
                    runAction(() =>
                      marketplaceApi.acquireProduct({
                        productId: detail.id,
                        licenseType: detail.licenseType,
                      }),
                    )
                  }
                >
                  本地演示获取
                </Button>
              )}
              {detail.selfOwned && <Tag color="orange">不能获取或绑定自己发布的商品</Tag>}
              {detail.acquired && !detail.installed && (
                <Button
                  type="primary"
                  loading={actionLoading}
                  disabled={detail.revoked}
                  onClick={() =>
                    runAction(() =>
                      marketplaceApi.installProduct({
                        productId: detail.id,
                        confirmPermissions: false,
                      }),
                    )
                  }
                >
                  安装
                </Button>
              )}
              {detail.installed && detail.hasUpdate && (
                <Button
                  icon={<RotateCcw size={16} />}
                  loading={actionLoading}
                  onClick={() =>
                    runAction(() =>
                      marketplaceApi.updateProduct({
                        productId: detail.id,
                        confirmAddedPermissions: false,
                      }),
                    )
                  }
                >
                  更新到 {detail.updateVersion}
                </Button>
              )}
              {detail.installed && !detail.enabled && (
                <Button
                  loading={actionLoading}
                  disabled={detail.revoked}
                  onClick={() => runAction(() => marketplaceApi.enableProduct(detail.id))}
                >
                  启用
                </Button>
              )}
              {detail.installed && detail.enabled && (
                <Button loading={actionLoading} onClick={() => runAction(() => marketplaceApi.disableProduct(detail.id))}>
                  禁用
                </Button>
              )}
              {detail.installed && detail.enabled && detail.deliveryMode && (
                <Button type="primary" loading={actionLoading} onClick={openServiceSetup}>
                  配置并测试服务
                </Button>
              )}
              {detail.installed && (
                <Button
                  danger
                  icon={<Trash2 size={16} />}
                  loading={actionLoading}
                  onClick={() => runAction(() => marketplaceApi.uninstallProduct(detail.id))}
                >
                  卸载
                </Button>
              )}
              {detail.installed && (
                <Button loading={actionLoading} onClick={() => mockTest(detail.id)}>
                  {isAiServiceProduct(detail) ? "演示自检" : "Mock测试"}
                </Button>
              )}
              {import.meta.env.DEV && (
                <>
                  <Button
                    danger
                    type="dashed"
                    loading={actionLoading}
                    onClick={() =>
                      runAction(() => marketplaceApi.devRevokeProductVersion(detail.id))
                    }
                  >
                    开发模式：吊销版本
                  </Button>
                  <Button
                    type="dashed"
                    loading={actionLoading}
                    onClick={() =>
                      runAction(() => marketplaceApi.devRestoreProductVersion(detail.id))
                    }
                  >
                    开发模式：恢复版本
                  </Button>
                </>
              )}
            </Space>
            <Divider />
            <Card size="small" title="评价">
              {reviews.length === 0 ? (
                <Empty description="暂无评价" />
              ) : (
                <Space direction="vertical" className="w-full">
                  {reviews.map((review) => (
                    <div key={review.id} className="rounded-lg border border-slate-100 bg-white p-3">
                      <Space wrap>
                        <Text strong>{review.buyerNickname}</Text>
                        <Tag color="gold">{review.rating}/5</Tag>
                        {review.verifiedPurchase && <Tag color="green">已验证获取</Tag>}
                        {review.orderRefunded && <Tag color="orange">订单已退款</Tag>}
                      </Space>
                      <Paragraph className="!mb-0 mt-2">{review.content}</Paragraph>
                    </div>
                  ))}
                </Space>
              )}
              {detail.acquired && !detail.selfOwned && (
                <>
                  <Divider />
                  <Form form={reviewForm} layout="vertical" onFinish={submitReview}>
                    <Form.Item name="orderId" label="订单" rules={[{ required: true, message: "请选择订单" }]}>
                      <Select
                        options={orders
                          .filter((order) => order.productId === detail.id && order.paymentStatus === "paid")
                          .map((order) => ({
                            value: order.id,
                            label: `#${order.id} v${order.versionSnapshot ?? "-"} ${order.refundStatus === "refund_success" ? "(refunded)" : ""}`,
                          }))}
                      />
                    </Form.Item>
                    <Form.Item name="rating" label="评分" initialValue={5} rules={[{ required: true }]}>
                      <Select options={[1, 2, 3, 4, 5].map((value) => ({ value, label: `${value}/5` }))} />
                    </Form.Item>
                    <Form.Item name="content" label="评价内容" rules={[{ required: true, whitespace: true }]}>
                      <Input.TextArea rows={3} maxLength={500} />
                    </Form.Item>
                    <Button htmlType="submit">提交评价</Button>
                  </Form>
                </>
              )}
            </Card>
          </div>
        )}
      </Drawer>
      <Modal
        title="历史本地演示订单（非真实支付）"
        open={ordersOpen}
        onCancel={() => setOrdersOpen(false)}
        footer={<Button onClick={() => setOrdersOpen(false)}>关闭</Button>}
        width={820}
      >
        {orders.length === 0 ? (
          <Empty description="当前账号暂无历史演示订单" />
        ) : (
          <Space direction="vertical" className="w-full">
            {orders.map((order) => (
              <Card
                key={order.id}
                size="small"
                title={`${order.productName} #${order.id}`}
                extra={<Tag color={order.isMock ? "blue" : "red"}>{order.isMock ? "mock" : "real?"}</Tag>}
              >
                <Descriptions size="small" column={2}>
                  <Descriptions.Item label="买家">{order.buyerUserId}</Descriptions.Item>
                  <Descriptions.Item label="卖家">{order.sellerUserId}</Descriptions.Item>
                  <Descriptions.Item label="版本快照">{order.versionSnapshot ?? "-"}</Descriptions.Item>
                  <Descriptions.Item label="历史展示金额">{priceFromCents(order.grossAmount)}</Descriptions.Item>
                  <Descriptions.Item label="历史模拟服务费">{priceFromCents(order.platformFee)}</Descriptions.Item>
                  <Descriptions.Item label="历史模拟收入">{priceFromCents(order.sellerIncome)}</Descriptions.Item>
                  <Descriptions.Item label="支付状态">{order.isMock ? `${order.paymentStatus}（本地演示）` : order.paymentStatus}</Descriptions.Item>
                  <Descriptions.Item label="退款状态">{order.refundStatus}</Descriptions.Item>
                </Descriptions>
                {order.paymentStatus === "paid" && order.refundStatus !== "refund_success" && (
                  <Button danger size="small" className="mt-3" onClick={() => requestRefund(order)}>
                    本地模拟退款
                  </Button>
                )}
              </Card>
            ))}
          </Space>
        )}
      </Modal>
      <Modal
        title={`配置 ${detail?.name ?? "AI 服务"}`}
        open={setupOpen}
        width={720}
        confirmLoading={actionLoading}
        okText={detail?.deliveryMode === "byok" ? "测试连接并创建智能体" : "保存并执行 Mock 测试"}
        onOk={saveServiceSetup}
        onCancel={() => {
          setupForm.resetFields();
          setSetupOpen(false);
        }}
        destroyOnHidden
      >
        <Form form={setupForm} layout="vertical">
          <Alert
            className="mb-4"
            type={detail?.deliveryMode === "byok" ? "warning" : "info"}
            showIcon
            message={detail?.deliveryMode === "byok" ? "测试会向讯飞发送最小输入“你好”，可能消耗少量额度" : "本阶段测试结果明确标记为 Mock"}
            description={detail?.deliveryMode === "byok"
              ? "API Key、API Secret 和 Flow ID 必须属于同一星辰应用授权。密钥保存后不会返回前端。"
              : `开发者：${detail?.developerName ?? "-"}；数据去向：${detail?.dataDestination ?? "未声明"}`}
          />
          <Form.Item name="endpoint" label={detail?.deliveryMode === "remote-mcp" ? "MCP Server URL" : "Endpoint"}>
            <Input readOnly />
          </Form.Item>
          <Form.Item name="credentialMode" label="凭据来源" rules={[{ required: true }]}>
            <Select options={[
              { value: "existing", label: "选择 AI 资源中心已有凭据" },
              { value: "new", label: "在本向导中新建安全凭据" },
              ...(detail?.deliveryMode === "remote-mcp" ? [{ value: "none", label: "无鉴权" }] : []),
            ]} />
          </Form.Item>
          {setupCredentialMode === "existing" && (
            <>
              <Form.Item
                name="credentialId"
                label="已有凭据"
                rules={detail?.deliveryMode === "byok" ? [{ required: true, message: "请选择讯飞 Workflow 凭据" }] : undefined}
              >
                <Select
                  allowClear={detail?.deliveryMode !== "byok"}
                  placeholder="仅显示已安全保存的凭据引用"
                  options={credentials
                    .filter((credential) => {
                      const provider = detail?.deliveryMode === "byok" ? "xingchen" : detail?.deliveryMode;
                      return !provider || credential.provider === provider;
                    })
                    .map((credential) => ({ value: credential.id, label: `${credential.label} ${credential.maskedHint ?? ""}` }))}
                />
              </Form.Item>
              <Button type="link" className="!px-0" onClick={() => navigate("/ai-resources")}>前往 AI 资源中心管理凭据</Button>
            </>
          )}
          {setupCredentialMode === "new" && (
            <>
              <Form.Item name="credentialLabel" label="凭据名称" rules={[{ required: true }]}><Input /></Form.Item>
              {detail?.deliveryMode === "byok" ? (
                <>
                  <Form.Item name="appId" label="APPID" rules={[{ required: true }]}><Input.Password autoComplete="off" /></Form.Item>
                  <Form.Item name="apiKey" label="API Key" rules={[{ required: true }]}><Input.Password autoComplete="off" /></Form.Item>
                  <Form.Item name="apiSecret" label="API Secret" rules={[{ required: true }]}><Input.Password autoComplete="off" /></Form.Item>
                </>
              ) : (
                <Form.Item name="bearerToken" label="服务访问 Token（可选）"><Input.Password autoComplete="off" /></Form.Item>
              )}
            </>
          )}
          {detail?.deliveryMode === "byok" && (
            <>
              <Form.Item name="agentName" label="智能体配置名称" rules={[{ required: true }]}><Input placeholder={detail.name} /></Form.Item>
              <Form.Item name="flowId" label="Flow ID" rules={[{ required: true, whitespace: true, message: "Flow ID 不能为空" }]}><Input /></Form.Item>
              <Form.Item name="inputParameter" label="开始节点参数名" rules={[{ required: true }]}>
                <Input placeholder="AGENT_USER_INPUT / question / input" />
              </Form.Item>
              <Form.List name="workflowInputFields">
                {(fields, { add, remove }) => (
                  <Form.Item
                    label="Workflow 开始节点字段"
                    extra="字段 key 会原样写入 parameters；来自商品 schema，可在旧商品缺失配置时手动调整。"
                  >
                    <Space direction="vertical" className="w-full" size={8}>
                      {fields.map((field) => (
                        <Space key={field.key} align="baseline" className="w-full" size={8}>
                          <Form.Item
                            {...field}
                            name={[field.name, "key"]}
                            rules={[{ required: true, message: "字段名不能为空" }]}
                          >
                            <Input placeholder="major / target / AGENT_USER_INPUT" style={{ width: 180 }} />
                          </Form.Item>
                          <Form.Item {...field} name={[field.name, "label"]}>
                            <Input placeholder="显示名称" style={{ width: 130 }} />
                          </Form.Item>
                          <Form.Item {...field} name={[field.name, "type"]}>
                            <Select style={{ width: 140 }} options={WORKFLOW_FIELD_TYPE_OPTIONS} />
                          </Form.Item>
                          <Form.Item {...field} name={[field.name, "required"]} valuePropName="checked">
                            <Checkbox>必填</Checkbox>
                          </Form.Item>
                          <Form.Item {...field} name={[field.name, "defaultValue"]}>
                            <Input placeholder="默认值（可选）" style={{ width: 150 }} />
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
                <Input placeholder="answer / content / text / result" />
              </Form.Item>
            </>
          )}
          {detail?.deliveryMode !== "byok" && (
            <Form.Item
              name="networkPermissionConfirmed"
              label="网络权限确认"
              valuePropName="checked"
              rules={[{ validator: (_, value) => value ? Promise.resolve() : Promise.reject(new Error("必须确认网络访问权限")) }]}
            >
              <Checkbox>我确认该商品将访问声明的第三方网络服务</Checkbox>
            </Form.Item>
          )}
          {detail?.deliveryMode === "remote-mcp" && (
            <Alert type="warning" showIcon message="远程 MCP Mock 注册" description="本阶段不会把 URL 当成本地命令启动，也不会执行 PowerShell、Shell、Python。" />
          )}
        </Form>
      </Modal>
    </div>
  );
}
