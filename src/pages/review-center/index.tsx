import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Divider,
  Empty,
  Input,
  Modal,
  Space,
  Table,
  Tag,
  Typography,
  message,
} from "antd";
import { CheckCircle2, CircleAlert, RefreshCw, ShieldCheck, ShieldX } from "lucide-react";
import { adminMarketplaceApi, marketplaceApi } from "@/lib/api";
import {
  CommerceHero,
  CommerceStatusTag,
  DeliveryModeTag,
  statusLabel,
} from "@/components/marketplace/CommerceShell";
import type {
  MarketplaceMockSession,
  MarketplaceReviewStatus,
  MarketplaceSubmission,
} from "@/types";

const { Text } = Typography;

const REVIEW_FILTERS: Array<{ value: MarketplaceReviewStatus | "all"; label: string }> = [
  { value: "all", label: "全部" },
  { value: "submitted", label: "待审核" },
  { value: "under_review", label: "审核中" },
  { value: "approved", label: "已批准" },
  { value: "rejected", label: "已驳回" },
  { value: "suspended", label: "已暂停" },
  { value: "delisted", label: "已下架" },
];

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
          <Text>同一用户可以完成本地审核、上架和安装验证；云端版本再接入真实审核权限。</Text>
        </Space>
      }
    />
  );
}

export default function ReviewCenterPage() {
  const [session, setSession] = useState<MarketplaceMockSession | null>(null);
  const [status, setStatus] = useState<MarketplaceReviewStatus | "all">("all");
  const [items, setItems] = useState<MarketplaceSubmission[]>([]);
  const [selected, setSelected] = useState<MarketplaceSubmission | null>(null);
  const [loading, setLoading] = useState(false);

  async function load() {
    const s = await marketplaceApi.getMockSession();
    setSession(s);
    if (s.canAdmin) {
      const list = await adminMarketplaceApi.listSubmissions(status === "all" ? null : status);
      setItems(list);
      setSelected((prev) => list.find((item) => item.id === prev?.id) ?? list[0] ?? null);
    } else {
      setItems([]);
      setSelected(null);
    }
  }

  useEffect(() => {
    load().catch((e) => message.error(String(e)));
  }, [status]);

  async function run(action: () => Promise<{ message: string }>) {
    setLoading(true);
    try {
      const result = await action();
      message.success(result.message);
      await load();
    } finally {
      setLoading(false);
    }
  }

  function askReason(title: string, cb: (reason: string) => Promise<{ message: string }>) {
    let reason = "";
    Modal.confirm({
      title,
      content: <Input.TextArea rows={3} placeholder="必须填写原因" onChange={(e) => { reason = e.target.value; }} />,
      okText: "确认",
      cancelText: "取消",
      onOk: async () => {
        if (!reason.trim()) {
          throw new Error("必须填写原因");
        }
        await run(() => cb(reason));
      },
    });
  }

  const selectedReport = selected?.scanReport;
  const selectedReady = Boolean(
    selectedReport?.ok
    && selectedReport.manifestValid
    && selectedReport.compatible
    && !selectedReport.hasExecutables
    && !selectedReport.hasScripts
    && !selectedReport.hasSuspectedSecrets,
  );
  const highRiskCount = items.filter((item) => {
    const report = item.scanReport;
    return Boolean(report && (report.hasExecutables || report.hasScripts || report.hasSuspectedSecrets || !report.compatible));
  }).length;

  if (session && !session.canAdmin) {
    return (
      <div className="commerce-page">
        <div className="commerce-page__inner">
        <RoleSwitcher session={session} onChanged={load} />
        <CommerceHero
          eyebrow="TRUST & SAFETY / RESTRICTED"
          title="审核工作区需要管理员权限"
          description="商品批准、驳回、暂停、下架和版本吊销都是受保护操作，普通买家和创作者不能访问审核队列。"
          icon={<ShieldCheck size={28} />}
          badge={<Tag color="orange">当前：{session.displayName}</Tag>}
          metrics={[
            { label: "队列", value: "不可见", hint: "避免泄漏审核信息", tone: "teal" },
            { label: "审批", value: "后端保护", hint: "不能用前端参数绕过", tone: "amber" },
            { label: "风险报告", value: "管理员可见", hint: "含脱敏扫描证据", tone: "blue" },
            { label: "操作审计", value: "保留", hint: "处置原因不可省略", tone: "coral" },
          ]}
        />
        <Alert className="mt-4" type="info" showIcon message="请在本地演示角色中切换到管理员" description="审核中心所有操作由 Rust 后端再次校验管理员角色。" />
        </div>
      </div>
    );
  }

  return (
    <div className="commerce-page">
      <div className="commerce-page__inner">
      <RoleSwitcher session={session} onChanged={load} />
      <CommerceHero
        eyebrow="TRUST & SAFETY / LOCAL REVIEW"
        title="审核与上架"
        description="先看证据，再做决策。Manifest、权限、密钥风险、运行时和数据去向在同一工作区完成复核。"
        icon={<ShieldCheck size={28} />}
        badge={<Tag color="red">本地审核</Tag>}
        actions={<Button icon={<RefreshCw size={15} />} loading={loading} onClick={load}>刷新队列</Button>}
        metrics={[
          { label: "当前队列", value: items.length, hint: status === "all" ? "全部提交" : statusLabel(status), tone: "teal" },
          { label: "待处理", value: items.filter((item) => ["submitted", "under_review"].includes(item.status)).length, hint: "需要审核决策", tone: "amber" },
          { label: "高风险", value: highRiskCount, hint: "脚本、密钥或兼容性", tone: "coral" },
          { label: "可批准", value: items.filter((item) => item.scanReport?.ok).length, hint: "仍需人工复核", tone: "blue" },
        ]}
      />

      <Alert
        className="my-4"
        type="warning"
        showIcon
        message="本地模拟审核不等于数字签名"
        description="批准后的商品仍标记 unsigned/local-demo；当前扫描只提供静态证据，不能替代正式恶意代码检测。"
      />

      <div className="commerce-panel commerce-filterbar">
        <div className="commerce-segments">
          {REVIEW_FILTERS.map((filter) => (
            <Button
              key={filter.value}
              className="commerce-segment"
              type={status === filter.value ? "primary" : "text"}
              onClick={() => setStatus(filter.value)}
            >
              {filter.label}
            </Button>
          ))}
        </div>
        <Text type="secondary">选择左侧提交后，在右侧完成证据复核与处置</Text>
      </div>

      <div className="commerce-master-detail">
        <Card className="commerce-table-card" title="审核队列" extra={<Tag>{items.length} 项</Tag>}>
          <Table
            rowKey="id"
            loading={loading}
            dataSource={items}
            pagination={{ pageSize: 8 }}
            onRow={(record) => ({ onClick: () => setSelected(record) })}
            columns={[
              { title: "商品", dataIndex: "productName" },
              { title: "版本", dataIndex: "version" },
              { title: "包格式", render: (_, row) => <Tag color={row.scanReport?.packageFormat === "v3-firstwork-plugin" ? "blue" : "default"}>{row.scanReport?.packageFormat ?? "未上传"}</Tag> },
              { title: "开发者", dataIndex: "developerName" },
              { title: "状态", render: (_, row) => <CommerceStatusTag status={row.status} /> },
              { title: "提交时间", dataIndex: "submittedAt" },
            ]}
          />
        </Card>
        <Card className="commerce-detail-card" title={selected?.productName ?? "审核详情"} extra={selected && <CommerceStatusTag status={selected.status} />}>
          {selected ? (
            <>
              <div className="commerce-review-readiness mb-4">
                <div className={`commerce-review-check ${selectedReport?.manifestValid ? "commerce-review-check--ok" : "commerce-review-check--risk"}`}>
                  {selectedReport?.manifestValid ? <CheckCircle2 size={15} /> : <CircleAlert size={15} />}
                  Manifest {selectedReport?.manifestValid ? "结构有效" : "需要修复"}
                </div>
                <div className={`commerce-review-check ${selectedReport?.compatible ? "commerce-review-check--ok" : "commerce-review-check--risk"}`}>
                  {selectedReport?.compatible ? <CheckCircle2 size={15} /> : <CircleAlert size={15} />}
                  版本 {selectedReport?.compatible ? "兼容" : "不兼容"}
                </div>
                <div className={`commerce-review-check ${selectedReport && !selectedReport.hasSuspectedSecrets ? "commerce-review-check--ok" : "commerce-review-check--risk"}`}>
                  {selectedReport && !selectedReport.hasSuspectedSecrets ? <CheckCircle2 size={15} /> : <ShieldX size={15} />}
                  {selectedReport?.hasSuspectedSecrets ? "发现疑似密钥" : "未发现密钥明文"}
                </div>
                <div className={`commerce-review-check ${selectedReport && !selectedReport.hasScripts && !selectedReport.hasExecutables ? "commerce-review-check--ok" : "commerce-review-check--risk"}`}>
                  {selectedReport && !selectedReport.hasScripts && !selectedReport.hasExecutables ? <CheckCircle2 size={15} /> : <ShieldX size={15} />}
                  {selectedReport?.hasScripts || selectedReport?.hasExecutables ? "包含脚本或可执行文件" : "声明式运行边界"}
                </div>
              </div>
              <Descriptions size="small" column={1}>
                <Descriptions.Item label="提交 ID">{selected.id}</Descriptions.Item>
                <Descriptions.Item label="商品 ID"><Text copyable>{selected.productId}</Text></Descriptions.Item>
                <Descriptions.Item label="商品">{selected.productName}</Descriptions.Item>
                <Descriptions.Item label="版本">{selected.version ?? "-"}</Descriptions.Item>
                <Descriptions.Item label="开发者">{selected.developerName}</Descriptions.Item>
                <Descriptions.Item label="状态"><CommerceStatusTag status={selected.status} /></Descriptions.Item>
                <Descriptions.Item label="审核信息">{selected.reviewMessage ?? "-"}</Descriptions.Item>
              </Descriptions>
              <Divider />
              {selected.scanReport ? (
                <>
                  <Descriptions size="small" column={1}>
                    <Descriptions.Item label="扫描状态"><CommerceStatusTag status={selected.scanReport.status} /></Descriptions.Item>
                    <Descriptions.Item label="包格式">{selected.scanReport.packageFormat}</Descriptions.Item>
                    <Descriptions.Item label="Manifest schema">v{selected.scanReport.schemaVersion ?? "-"}</Descriptions.Item>
                    <Descriptions.Item label="插件 ID"><Text copyable>{selected.scanReport.pluginId ?? "-"}</Text></Descriptions.Item>
                    <Descriptions.Item label="插件分类">{selected.scanReport.classification ?? "legacy"}</Descriptions.Item>
                    <Descriptions.Item label="运行时">{selected.scanReport.runtimeKind ?? "-"}</Descriptions.Item>
                    <Descriptions.Item label="Feature 入口">{selected.scanReport.features.join("、") || "-"}</Descriptions.Item>
                    <Descriptions.Item label="Enhancement hooks">{selected.scanReport.enhancementHooks.join("、") || "-"}</Descriptions.Item>
                    <Descriptions.Item label="适用场景">{selected.scanReport.supportedScenes.join("、") || "-"}</Descriptions.Item>
                    <Descriptions.Item label="交付方式"><DeliveryModeTag mode={selected.scanReport.deliveryMode} /></Descriptions.Item>
                    <Descriptions.Item label="协议">{selected.scanReport.protocol ?? "-"}</Descriptions.Item>
                    <Descriptions.Item label="权限">{selected.scanReport.permissions.join(", ") || "-"}</Descriptions.Item>
                    <Descriptions.Item label="凭据要求">
                      {selected.scanReport.credentialRequirements.length > 0
                        ? selected.scanReport.credentialRequirements.map((item) => `${item.label ?? item.id}${item.provider ? `（${item.provider}）` : ""}`).join("、")
                        : "无"}
                    </Descriptions.Item>
                    <Descriptions.Item label="SHA-256"><Text copyable>{selected.scanReport.sha256}</Text></Descriptions.Item>
                    <Descriptions.Item label="签名">unsigned，本地审核不会伪装成官方签名</Descriptions.Item>
                  </Descriptions>
                  {selected.scanReport.packageFormat === "v3-firstwork-plugin" && (
                    <Alert
                      className="mt-3"
                      type="warning"
                      showIcon
                      message="可信公钥验签尚未接入"
                      description="本地审核会锁定已批准包的 SHA-256，并在市场安装前复验；unsigned 仅表示未接入正式签名服务，不代表已获得平台数字签名。"
                    />
                  )}
                  {selected.scanReport.warnings.length > 0 && (
                    <Alert className="mt-3" type="warning" showIcon message="预检警告" description={selected.scanReport.warnings.join("；")} />
                  )}
                  {selected.scanReport.findings.length > 0 && (
                    <Alert className="mt-3" type="warning" showIcon message="静态风险" description={selected.scanReport.findings.map((f) => `${f.category}: ${f.file}`).join("；")} />
                  )}
                  {selected.scanReport.deliveryMode && (
                    <Alert
                      className="mt-3"
                      type={selected.scanReport.hasSuspectedSecrets ? "error" : "info"}
                      showIcon
                      message="AI 服务交付安全复核"
                      description={
                        selected.scanReport.deliveryMode === "byok"
                          ? "BYOK 包不得携带开发者 API Key/API Secret；用户凭据必须由 AI 资源中心保存。"
                          : selected.scanReport.deliveryMode === "hosted-api"
                            ? "确认 Endpoint 为 HTTPS，并复核开发者、数据去向、鉴权方式及配置变化。"
                            : "确认 Remote MCP 声明 mcp.connect、network.request、访问域名和工具能力。"
                      }
                    />
                  )}
                </>
              ) : <Empty description="暂无扫描报告" />}
              <Divider />
              <Space wrap>
                {selected.status === "submitted" && (
                  <Button type="primary" onClick={() => run(() => adminMarketplaceApi.startReview({ submissionId: selected.id, message: "开始本地模拟审核" }))}>开始审核</Button>
                )}
                {selected.status === "under_review" && (
                  <>
                    <Button type="primary" disabled={!selectedReady} onClick={() => run(() => adminMarketplaceApi.approveSubmission({ submissionId: selected.id, message: "本地模拟审核通过" }))}>批准发布</Button>
                    <Button danger onClick={() => askReason("驳回原因", (reason) => adminMarketplaceApi.rejectSubmission({ submissionId: selected.id, message: reason }))}>驳回</Button>
                  </>
                )}
                {["approved", "published"].includes(selected.status) && (
                  <Button onClick={() => askReason("暂停原因", (reason) => adminMarketplaceApi.suspendProduct({ productId: selected.productId, reason }))}>暂停商品</Button>
                )}
                {["suspended", "delisted"].includes(selected.status) && (
                  <Button onClick={() => askReason("恢复原因", (reason) => adminMarketplaceApi.restoreProduct({ productId: selected.productId, reason }))}>恢复商品</Button>
                )}
                {["approved", "published", "suspended"].includes(selected.status) && (
                  <>
                    <Button danger onClick={() => askReason("下架原因", (reason) => adminMarketplaceApi.delistProduct({ productId: selected.productId, reason }))}>下架商品</Button>
                    <Button danger onClick={() => askReason("吊销版本原因", (reason) => adminMarketplaceApi.revokeVersion({ productId: selected.productId, version: selected.version, reason }))}>吊销此版本</Button>
                  </>
                )}
              </Space>
              {selected.status === "under_review" && !selectedReady && (
                <Alert className="mt-3" type="error" showIcon message="当前证据不满足批准条件" description="请先处理 Manifest、兼容性、脚本/可执行文件或疑似密钥风险。" />
              )}
            </>
          ) : <Empty description="暂无审核项" />}
        </Card>
      </div>
      </div>
    </div>
  );
}
