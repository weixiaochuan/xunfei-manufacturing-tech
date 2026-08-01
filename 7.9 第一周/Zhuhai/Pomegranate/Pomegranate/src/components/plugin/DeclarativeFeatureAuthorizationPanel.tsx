import { useCallback, useEffect, useState } from "react";
import { Alert, Button, Empty, List, Popconfirm, Space, Spin, Tag, Typography, message } from "antd";
import { expirationFromDuration } from "@/lib/api/exactAuthorization";
import { featureAuthorizationApi, type FeatureAuthorizationView } from "@/lib/api/featureAuthorization";

const { Text } = Typography;
const statusColor = (status: FeatureAuthorizationView["status"]) => status === "granted" ? "green" : status === "missing" || status === "pending" ? "orange" : "red";

export function DeclarativeFeatureAuthorizationPanel({ pluginId, canAuthorize }: { pluginId: string; canAuthorize: boolean }) {
  const [items, setItems] = useState<FeatureAuthorizationView[]>([]);
  const [loading, setLoading] = useState(true);
  const [acting, setActing] = useState<string>();
  const refresh = useCallback(async () => {
    setLoading(true);
    try { setItems(await featureAuthorizationApi.list(pluginId)); }
    catch (error) { setItems([]); message.error(`加载声明式功能授权失败：${String(error)}`); }
    finally { setLoading(false); }
  }, [pluginId]);
  useEffect(() => { void refresh(); }, [refresh]);

  async function mutate(item: FeatureAuthorizationView, action: "grant" | "deny" | "revoke" | "expire") {
    setActing(item.contributionId);
    try {
      if (action === "grant") await featureAuthorizationApi.grant(pluginId, item.contributionId, expirationFromDuration(24));
      else await featureAuthorizationApi[action](pluginId, item.contributionId);
      await refresh();
      message.success("声明式功能授权状态已更新");
    } catch (error) { message.error(`更新声明式功能授权失败：${String(error)}`); }
    finally { setActing(undefined); }
  }

  const grantPrompt = (item: FeatureAuthorizationView) => item.capabilityId === "ai.invoke"
    ? "允许此星辰功能调用 AI 吗？"
    : "允许此贡献增强 AI 上下文吗？";

  if (loading) return <Spin size="small" />;
  if (items.length === 0) return <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="当前版本没有可授权的上下文增强贡献" />;
  return <Space orientation="vertical" style={{ width: "100%" }} size="middle">
    <Alert type="warning" showIcon title="声明式上下文增强授权" description="范围由后端从当前 Manifest 的场景、功能与 hook 构造；页面不会提交 scope，也不会直接执行插件内容。" />
    <List size="small" dataSource={items} renderItem={(item) => <List.Item actions={[
      <Popconfirm key="grant" title={grantPrompt(item)} onConfirm={() => mutate(item, "grant")} disabled={!canAuthorize || item.effective}>
        <Button size="small" type="primary" disabled={!canAuthorize || item.effective} loading={acting === item.contributionId}>授权 24 小时</Button>
      </Popconfirm>,
      <Button key="deny" size="small" disabled={!canAuthorize || item.status === "denied"} onClick={() => mutate(item, "deny")}>拒绝</Button>,
      <Button key="revoke" size="small" danger disabled={item.status !== "granted"} onClick={() => mutate(item, "revoke")}>撤销</Button>,
      <Button key="expire" size="small" disabled={!item.expiresAt || new Date(item.expiresAt) > new Date()} onClick={() => mutate(item, "expire")}>确认过期</Button>,
    ]}>
      <Space orientation="vertical" size={2}>
        <Space wrap><Text strong>{item.title}</Text><Tag>{item.capabilityId}</Tag><Tag>{item.hook}</Tag><Tag color={statusColor(item.status)}>{item.status}</Tag></Space>
        <Text type="secondary">场景：{item.scenes.join("、") || "未指定"}；功能：{item.features.join("、") || "未指定"}</Text>
        {item.expiresAt && <Text type="secondary">到期：{item.expiresAt}</Text>}
      </Space>
    </List.Item>} />
    {!canAuthorize && <Alert type="error" showIcon title="插件当前不可授权，请先恢复有效安装、完整性与启用状态。" />}
  </Space>;
}
