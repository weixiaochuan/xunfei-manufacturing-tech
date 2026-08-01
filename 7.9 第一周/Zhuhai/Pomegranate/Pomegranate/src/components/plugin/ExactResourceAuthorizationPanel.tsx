import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Empty,
  List,
  Popconfirm,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
  message,
} from "antd";
import { PLUGIN_CAPABILITY_PRESENTATION } from "@/generated/pluginCapabilities";
import {
  exactAuthorizationApi,
  expirationFromDuration,
  resourceSelectionValue,
  resourcesForCapability,
  type ExactAuthorizationCatalog,
  type ExactAuthorizationResourceOption,
  type ExactAuthorizationView,
} from "@/lib/api/exactAuthorization";

const { Text } = Typography;
const DURATIONS = [
  { value: 1, label: "1 小时" },
  { value: 8, label: "8 小时" },
  { value: 24, label: "24 小时" },
] as const;

function resourceKey(resource: ExactAuthorizationResourceOption, capabilityId: string) {
  return `${capabilityId}\u0000${resource.resourceKind}\u0000${resource.resourceId}`;
}

function statusColor(status: ExactAuthorizationView["status"]) {
  return status === "granted" ? "green" : status === "missing" || status === "pending" ? "orange" : "red";
}

export function ExactResourceAuthorizationPanel({
  pluginId,
  canAuthorize,
}: {
  pluginId: string;
  canAuthorize: boolean;
}) {
  const [catalog, setCatalog] = useState<ExactAuthorizationCatalog | null>(null);
  const [history, setHistory] = useState<ExactAuthorizationView[]>([]);
  const [states, setStates] = useState<Record<string, ExactAuthorizationView>>({});
  const [capabilityId, setCapabilityId] = useState<string>();
  const [resourceSelection, setResourceSelection] = useState<string>();
  const [duration, setDuration] = useState<1 | 8 | 24>(1);
  const [loading, setLoading] = useState(false);
  const [acting, setActing] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [nextCatalog, nextHistory] = await Promise.all([
        exactAuthorizationApi.catalog(pluginId),
        exactAuthorizationApi.list(pluginId),
      ]);
      const queries = nextCatalog.capabilityIds.flatMap((capability) =>
        resourcesForCapability(nextCatalog, capability).map(async (resource) => [
          resourceKey(resource, capability),
          await exactAuthorizationApi.query({
            pluginId,
            capabilityId: capability,
            resourceKind: resource.resourceKind,
            resourceId: resource.resourceId,
          }),
        ] as const),
      );
      setCatalog(nextCatalog);
      setHistory(nextHistory);
      setStates(Object.fromEntries(await Promise.all(queries)));
      setCapabilityId((current) =>
        current && nextCatalog.capabilityIds.includes(current)
          ? current
          : nextCatalog.capabilityIds[0],
      );
    } catch (error) {
      setCatalog(null);
      setHistory([]);
      setStates({});
      message.error(`加载具体资源授权失败：${String(error)}`);
    } finally {
      setLoading(false);
    }
  }, [pluginId]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const resources = useMemo(
    () => (catalog && capabilityId ? resourcesForCapability(catalog, capabilityId) : []),
    [catalog, capabilityId],
  );
  const selectedResource = resources.find(
    (resource) => resourceSelectionValue(resource) === resourceSelection,
  );
  const selectedState = selectedResource && capabilityId
    ? states[resourceKey(selectedResource, capabilityId)]
    : undefined;
  const knownHandles = new Set(
    Object.values(states).flatMap((state) => state.authorizationId ? [state.authorizationId] : []),
  );
  const unavailableHistory = history.filter(
    (authorization) => authorization.authorizationId && !knownHandles.has(authorization.authorizationId),
  );

  async function grant() {
    if (!capabilityId || !selectedResource) return;
    setActing(true);
    try {
      await exactAuthorizationApi.grant({
        pluginId,
        capabilityId,
        resourceKind: selectedResource.resourceKind,
        resourceId: selectedResource.resourceId,
        expiresAt: expirationFromDuration(duration),
      });
      await refresh();
      message.success("具体资源授权已写入");
    } catch (error) {
      message.error(`授权失败：${String(error)}`);
    } finally {
      setActing(false);
    }
  }

  async function revoke(authorizationId: string) {
    setActing(true);
    try {
      await exactAuthorizationApi.revoke(pluginId, authorizationId);
      await refresh();
      message.success("具体资源授权已撤销");
    } catch (error) {
      message.error(`撤销失败：${String(error)}`);
    } finally {
      setActing(false);
    }
  }

  if (loading && !catalog) return <Spin size="small" />;
  if (!catalog || catalog.capabilityIds.length === 0) {
    return <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="当前版本没有可配置的具体资源权限" />;
  }

  const presentation = capabilityId
    ? PLUGIN_CAPABILITY_PRESENTATION[capabilityId as keyof typeof PLUGIN_CAPABILITY_PRESENTATION]
    : undefined;

  return (
    <Space orientation="vertical" style={{ width: "100%" }} size="middle">
      <Alert
        type="warning"
        showIcon
        title="具体资源授权"
        description="权限声明不等于真实授权。这里只授权所选资源，最长 24 小时；资源所有权、宿主安装和 exact scope 均由后端重新验证。"
      />
      <Space wrap>
        <Select
          aria-label="具体资源能力"
          style={{ minWidth: 190 }}
          value={capabilityId}
          options={catalog.capabilityIds.map((id) => ({
            value: id,
            label: PLUGIN_CAPABILITY_PRESENTATION[id as keyof typeof PLUGIN_CAPABILITY_PRESENTATION]?.title ?? id,
          }))}
          onChange={(value) => {
            setCapabilityId(value);
            setResourceSelection(undefined);
          }}
        />
        <Select
          aria-label="具体资源"
          style={{ minWidth: 240 }}
          value={resourceSelection}
          placeholder="选择具体资源"
          options={resources.map((resource) => ({
            value: resourceSelectionValue(resource),
            label: `${resource.displayName} · ${resource.resourceKind}`,
          }))}
          onChange={setResourceSelection}
        />
        <Select
          aria-label="授权有效期"
          value={duration}
          options={DURATIONS.map((item) => ({ ...item }))}
          onChange={setDuration}
        />
        <Popconfirm
          title="确认授权此具体资源？"
          description="授权不会立即调用资源，后续生产调用仍需通过统一 Guard。"
          onConfirm={grant}
          disabled={!canAuthorize || !selectedResource || selectedState?.effective}
        >
          <Button
            type="primary"
            loading={acting}
            disabled={!canAuthorize || !selectedResource || selectedState?.effective}
          >
            授权所选资源
          </Button>
        </Popconfirm>
      </Space>
      {presentation && (
        <Text type="secondary">
          风险 {presentation.riskLevel}：{presentation.description}
        </Text>
      )}
      {!canAuthorize && <Alert type="error" showIcon title="插件当前不可授权，请先恢复有效安装与启用状态。" />}
      <List
        size="small"
        dataSource={Object.entries(states)}
        locale={{ emptyText: "暂无可授权资源" }}
        renderItem={([key, authorization]) => {
          const [capability] = key.split("\u0000");
          const resource = catalog.resources.find((item) =>
            key === resourceKey(item, capability),
          );
          return (
            <List.Item
              actions={authorization.authorizationId ? [
                <Popconfirm
                  key="revoke"
                  title="确认撤销此资源授权？"
                  onConfirm={() => revoke(authorization.authorizationId!)}
                >
                  <Button size="small" danger loading={acting}>撤销</Button>
                </Popconfirm>,
              ] : []}
            >
              <Space wrap>
                <Text>{resource?.displayName ?? "资源不可用"}</Text>
                <Tag>{resource?.resourceKind ?? authorization.resourceKind}</Tag>
                <Tag>{capability}</Tag>
                <Tag color={statusColor(authorization.status)}>{authorization.status}</Tag>
                {authorization.expiresAt && <Text type="secondary">至 {authorization.expiresAt}</Text>}
              </Space>
            </List.Item>
          );
        }}
      />
      {unavailableHistory.length > 0 && (
        <List
          header={<Text strong>历史不可用资源授权</Text>}
          size="small"
          dataSource={unavailableHistory}
          renderItem={(authorization) => (
            <List.Item actions={authorization.authorizationId ? [
              <Popconfirm
                key="revoke-history"
                title="确认撤销此历史授权？"
                onConfirm={() => revoke(authorization.authorizationId!)}
              >
                <Button size="small" danger loading={acting}>撤销</Button>
              </Popconfirm>,
            ] : []}>
              <Space wrap>
                <Text>资源不可用</Text>
                <Tag>{authorization.resourceKind}</Tag>
                <Tag>{authorization.capabilityId}</Tag>
                <Tag color={statusColor(authorization.status)}>{authorization.status}</Tag>
              </Space>
            </List.Item>
          )}
        />
      )}
    </Space>
  );
}
