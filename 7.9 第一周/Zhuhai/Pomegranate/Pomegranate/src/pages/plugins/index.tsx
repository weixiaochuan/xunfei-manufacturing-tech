import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Empty,
  Modal,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useNavigate } from "react-router-dom";
import {
  AlertTriangle,
  FolderPlus,
  Plug,
  RefreshCw,
  RotateCw,
  ShieldCheck,
  ShieldOff,
  Trash2,
} from "lucide-react";
import { pluginApi } from "@/lib/api";
import type {
  PluginActivationRule,
  PluginArchiveInspection,
  PluginInfo,
  PluginAuditLogEntry,
  PluginDocumentSummaryConfig,
  PluginFeatureContributionV3,
  PluginScene,
  PluginVersionInfo,
} from "@/types";
import { pluginManager } from "@/services/pluginManager";
import { notifyDeclarativePluginToolbarChanged } from "@/services/declarativePluginEvents";
import { PLUGIN_CAPABILITY_PRESENTATION } from "@/generated/pluginCapabilities";

const { Text, Paragraph } = Typography;

/** T25: 插件审计日志内联面板 */
function PluginAuditLog({ pluginId }: { pluginId: string }) {
  const [logs, setLogs] = useState<PluginAuditLogEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [expanded, setExpanded] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await pluginApi.getAuditLog(pluginId, 20);
      setLogs(data);
    } catch {
      setLogs([]);
    } finally {
      setLoading(false);
    }
  }, [pluginId]);

  useEffect(() => {
    if (expanded && logs.length === 0) void load();
  }, [expanded]);

  return (
    <div style={{ marginTop: 8 }}>
      <Button
        size="small"
        type="link"
        onClick={() => setExpanded(!expanded)}
        loading={loading}
      >
        {expanded ? "收起" : "展开"}
      </Button>
      {expanded && (
        <div
          style={{
            maxHeight: 200,
            overflowY: "auto",
            marginTop: 4,
            fontSize: 11,
            color: "var(--ant-color-text-secondary)",
          }}
        >
          {logs.length === 0 && !loading && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              无审计记录
            </Text>
          )}
          {logs.map((e) => (
            <div
              key={e.id}
              style={{
                padding: "2px 0",
                borderBottom: "1px solid var(--ant-color-border-secondary)",
              }}
            >
              <span style={{ opacity: 0.6 }}>{e.timestamp.slice(0, 19)}</span>
              {" · "}
              <span style={{ fontWeight: 500 }}>{e.operation}</span>
              {e.target && (
                <>
                  {" → "}
                  <code style={{ fontSize: 10 }}>{e.target}</code>
                </>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function DocumentSummaryPluginSettings({ pluginId }: { pluginId: string }) {
  const navigate = useNavigate();
  const [config, setConfig] = useState<PluginDocumentSummaryConfig | null>(null);
  const [mode, setMode] = useState<"mock" | "agent">("mock");
  const [externalAgentId, setExternalAgentId] = useState<string | undefined>();
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const row = await pluginApi.getDocumentSummaryConfig(pluginId);
      setConfig(row);
      setMode(row.mode === "agent" ? "agent" : "mock");
      setExternalAgentId(row.externalAgentId ?? undefined);
    } catch (err) {
      message.error(`加载摘要插件设置失败：${String(err)}`);
      setConfig(null);
    } finally {
      setLoading(false);
    }
  }, [pluginId]);

  useEffect(() => {
    void load();
  }, [load]);

  async function save() {
    if (mode === "agent" && !externalAgentId) {
      message.warning("请选择一个已配置并可用的摘要智能体");
      return;
    }
    setSaving(true);
    try {
      const next = await pluginApi.setDocumentSummaryConfig({
        pluginId,
        mode,
        externalAgentId: mode === "agent" ? externalAgentId ?? null : null,
      });
      setConfig(next);
      setMode(next.mode === "agent" ? "agent" : "mock");
      setExternalAgentId(next.externalAgentId ?? undefined);
      message.success("摘要插件设置已保存");
    } catch (err) {
      message.error(`保存摘要插件设置失败：${String(err)}`);
    } finally {
      setSaving(false);
    }
  }

  const agents = config?.availableAgents ?? [];

  return (
    <Card
      size="small"
      title="AI 文档摘要设置"
      loading={loading}
      extra={<Button size="small" onClick={load}>刷新</Button>}
    >
      <Space direction="vertical" size={12} style={{ width: "100%" }}>
        <Alert
          type={mode === "mock" ? "warning" : "info"}
          showIcon
          message={mode === "mock" ? "当前为 Mock 演示模式" : "当前将通过统一智能体服务生成摘要"}
          description={
            mode === "mock"
              ? "Mock 模式不会调用真实 AI，也不会消耗星辰额度。切换到真实智能体后，插件仍只能保存 externalAgentId，不能读取凭据明文。"
              : "摘要请求会先经过 Rust 后端的插件权限、商品授权和智能体可用性检查，再由 AI 资源中心的 Provider 发起调用。"
          }
        />
        <Space wrap>
          <Text strong>摘要来源</Text>
          <Select
            style={{ width: 220 }}
            value={mode}
            onChange={(value: "mock" | "agent") => setMode(value)}
            options={[
              { value: "mock", label: "Mock 演示模式" },
              { value: "agent", label: "AI 资源中心智能体" },
            ]}
          />
          {mode === "agent" && (
            <Select
              style={{ minWidth: 320 }}
              placeholder="选择摘要智能体"
              value={externalAgentId}
              onChange={setExternalAgentId}
              options={agents.map((agent) => ({
                value: agent.id,
                label: `${agent.name}${agent.mockMode ? "（Mock）" : "（真实 Provider）"}`,
              }))}
            />
          )}
          <Button type="primary" loading={saving} onClick={save}>
            保存设置
          </Button>
        </Space>
        {mode === "agent" && agents.length === 0 && (
          <Alert
            type="warning"
            showIcon
            message="请先前往 AI 资源中心配置智能体。"
            description="下拉框只会显示已启用、凭据有效、商品授权有效且支持文本输入输出的智能体。"
            action={<Button size="small" onClick={() => navigate("/ai-resources")}>前往 AI 资源中心</Button>}
          />
        )}
        {agents.length > 0 && (
          <Space size={[0, 6]} wrap>
            {agents.map((agent) => (
              <Tag key={agent.id} color={agent.mockMode ? "blue" : "gold"}>
                {agent.name} / {agent.protocolType}
              </Tag>
            ))}
          </Space>
        )}
      </Space>
    </Card>
  );
}

const ACTIVATION_SCENES: Array<{ key: PluginScene; label: string }> = [
  { key: "learning", label: "学习场景" },
  { key: "research", label: "科研场景" },
  { key: "teaching", label: "教学场景" },
];

function PluginVersionActivationPanel({
  pluginId,
  onChanged,
}: {
  pluginId: string;
  onChanged: () => Promise<unknown>;
}) {
  const [versions, setVersions] = useState<PluginVersionInfo[]>([]);
  const [rules, setRules] = useState<PluginActivationRule[]>([]);
  const [loading, setLoading] = useState(true);
  const [changing, setChanging] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [nextVersions, nextRules] = await Promise.all([
        pluginApi.listVersions(pluginId),
        pluginApi.getActivationSettings(pluginId),
      ]);
      setVersions(nextVersions);
      setRules(nextRules);
    } catch {
      setVersions([]);
      setRules([]);
    } finally {
      setLoading(false);
    }
  }, [pluginId]);

  useEffect(() => {
    void load();
  }, [load]);

  function ruleValue(scopeType: "global" | "scene", scopeKey: string) {
    return rules.find((rule) => rule.scopeType === scopeType && rule.scopeKey === scopeKey)?.enabled ?? false;
  }

  async function setRule(scopeType: "global" | "scene", scopeKey: string, enabled: boolean) {
    const key = `${scopeType}:${scopeKey}`;
    setChanging(key);
    try {
      await pluginApi.setActivationSetting(pluginId, scopeType, scopeKey, enabled);
      setRules((current) => [
        ...current.filter((rule) => !(rule.scopeType === scopeType && rule.scopeKey === scopeKey)),
        { pluginId, scopeType, scopeKey, enabled, source: "user" },
      ]);
      await onChanged();
      message.success("启用范围已更新");
    } catch (error) {
      message.error(`更新启用范围失败：${String(error)}`);
    } finally {
      setChanging(null);
    }
  }

  function confirmRollback(version: PluginVersionInfo) {
    Modal.confirm({
      title: `回滚到 ${version.version}？`,
      content: "将原子切换当前版本；已安装的其他版本会保留，可再次切换。",
      okText: "确认回滚",
      cancelText: "取消",
      async onOk() {
        await pluginApi.rollback(pluginId, version.version);
        message.success(`已切换到 ${version.version}`);
        await Promise.all([load(), onChanged()]);
      },
    });
  }

  if (!loading && versions.length === 0) return null;

  return (
    <Card size="small" title="版本与启用范围" loading={loading}>
      <Space direction="vertical" size={12} style={{ width: "100%" }}>
        <Space wrap size={[8, 8]}>
          {versions.map((version) => (
            <Tag
              key={version.version}
              color={version.isCurrent ? "green" : "default"}
              closable={!version.isCurrent}
              closeIcon={<RotateCw size={11} />}
              onClose={(event) => {
                event.preventDefault();
                confirmRollback(version);
              }}
            >
              {version.version}{version.isCurrent ? "（当前）" : ""}
            </Tag>
          ))}
        </Space>
        <Space wrap size="large">
          <Space>
            <Text>全局启用</Text>
            <Switch
              checked={ruleValue("global", "")}
              loading={changing === "global:"}
              onChange={(enabled) => setRule("global", "", enabled)}
            />
          </Space>
          {ACTIVATION_SCENES.map((scene) => (
            <Space key={scene.key}>
              <Text>{scene.label}</Text>
              <Switch
                checked={ruleValue("scene", scene.key)}
                loading={changing === `scene:${scene.key}`}
                onChange={(enabled) => setRule("scene", scene.key, enabled)}
              />
            </Space>
          ))}
        </Space>
      </Space>
    </Card>
  );
}

function PluginFeatureLinks({ pluginId }: { pluginId: string }) {
  const navigate = useNavigate();
  const [features, setFeatures] = useState<PluginFeatureContributionV3[]>([]);
  const [rules, setRules] = useState<PluginActivationRule[]>([]);

  useEffect(() => {
    let cancelled = false;
    const scenes: PluginScene[] = ["global", "learning", "research", "teaching"];
    Promise.all([
      Promise.all(scenes.map((scene) => pluginApi.resolveEnabledContributions({
        scene,
        feature: "plugin-management",
        requestId: `plugin-links-${pluginId}-${scene}`,
        selectedResources: [],
        metadata: {},
        sessionOverrides: {},
      }))),
      pluginApi.getActivationSettings(pluginId),
    ]).then(([results, activationRules]) => {
      if (cancelled) return;
      const unique = new Map<string, PluginFeatureContributionV3>();
      for (const feature of results.flatMap((result) => result.features)) {
        if (feature.pluginId === pluginId) unique.set(feature.id, feature);
      }
      setFeatures([...unique.values()]);
      setRules(activationRules);
    }).catch(() => setFeatures([]));
    return () => { cancelled = true; };
  }, [pluginId]);

  if (features.length === 0) return null;
  async function setFeatureEnabled(featureId: string, enabled: boolean) {
    try {
      await pluginApi.setActivationSetting(pluginId, "feature", featureId, enabled);
      setRules((current) => [
        ...current.filter((rule) => !(rule.scopeType === "feature" && rule.scopeKey === featureId)),
        { pluginId, scopeType: "feature", scopeKey: featureId, enabled, source: "user" },
      ]);
    } catch (error) {
      message.error(`更新功能开关失败：${String(error)}`);
    }
  }

  return (
    <Card size="small" title="插件功能入口">
      <Space wrap>
        {features.map((feature) => (
          <Space key={feature.id} size={6}>
            <Button onClick={() => navigate(`/plugins/${pluginId}/features/${feature.id}`)}>
              {feature.title}
            </Button>
            <Tooltip title="仅控制这一项功能">
              <Switch
                size="small"
                checked={rules.find((rule) => rule.scopeType === "feature" && rule.scopeKey === feature.id)?.enabled ?? true}
                onChange={(enabled) => setFeatureEnabled(feature.id, enabled)}
              />
            </Tooltip>
          </Space>
        ))}
      </Space>
    </Card>
  );
}


/** 插件错误日志面板：从 PluginManager 拉取，按插件分组、可折叠 */
function PluginErrorLog() {
  const [logs, setLogs] = useState<ReturnType<typeof pluginManager.getErrorLog>>([]);
  const refresh = () => setLogs(pluginManager.getErrorLog());
  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, []);

  if (logs.length === 0) return null;

  type LogEntry = { pluginId: string; time: string; kind: string; message: string };
  const byPlugin = new Map<string, LogEntry[]>();
  for (const e of logs) {
    const bucket = byPlugin.get(e.pluginId);
    if (bucket) {
      bucket.push({ ...e });
    } else {
      byPlugin.set(e.pluginId, [{ ...e }]);
    }
  }

  return (
    <div style={{ marginTop: 16 }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 6,
          marginBottom: 8,
          color: "var(--ant-color-text-secondary)",
          fontSize: 13,
          fontWeight: 500,
        }}
      >
        <AlertTriangle size={14} style={{ color: "var(--ant-color-warning)" }} />
        错误日志（{logs.length} 条）
        <Button size="small" type="text" onClick={refresh} style={{ marginLeft: "auto" }}>
          <RefreshCw size={12} />
        </Button>
      </div>
      {Array.from(byPlugin.entries()).map(([pluginId, entries]) => (
        <div
          key={pluginId}
          style={{
            marginBottom: 8,
            padding: "8px 12px",
            background: "var(--ant-color-bg-container)",
            borderRadius: 6,
            border: "1px solid var(--ant-color-border-secondary)",
          }}
        >
          <Text strong style={{ fontSize: 12 }}>
            {pluginManager.getPluginName(pluginId) ?? pluginId}
          </Text>
          {entries.slice(-10).map((e, i) => (
            <div
              key={i}
              style={{
                fontSize: 11,
                color: "var(--ant-color-text-tertiary)",
                marginTop: 4,
                paddingLeft: 8,
                borderLeft: "2px solid var(--ant-color-warning)",
              }}
            >
              <span style={{ opacity: 0.6 }}>
                {e.time.slice(11, 19)} {e.kind}
              </span>
              <br />
              {e.message.slice(0, 200)}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}

function permissionLabel(permission: string) {
  return (PLUGIN_CAPABILITY_PRESENTATION as Record<string, { title: string }>)[permission]?.title
    ?? permission;
}

function permissionDescription(permission: string) {
  const presentation = (
    PLUGIN_CAPABILITY_PRESENTATION as Record<
      string,
      { description: string; riskLevel: string; status: string }
    >
  )[permission];
  return presentation
    ? `${presentation.description}（风险：${presentation.riskLevel}；状态：${presentation.status}）`
    : permission;
}

function statusColor(status: string) {
  switch (status) {
    case "installed":
      return "blue";
    case "invalid":
      return "red";
    case "error":
      return "volcano";
    default:
      return "default";
  }
}

function signatureColor(status: string) {
  switch (status) {
    case "valid":
      return "green";
    case "invalid":
    case "revoked":
      return "red";
    default:
      return "default";
  }
}

export default function PluginsPage() {
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [selected, setSelected] = useState<PluginInfo | null>(null);

  useEffect(() => {
    void scanPlugins();
  }, []);

  async function loadPlugins() {
    setLoading(true);
    try {
      const data = await pluginApi.list();
      setPlugins(data);
      return data;
    } catch (e) {
      message.error(`加载插件失败：${e}`);
      return [];
    } finally {
      setLoading(false);
    }
  }

  async function scanPlugins() {
    setLoading(true);
    try {
      const data = await pluginApi.scan();
      setPlugins(data);
      if (selected) {
        setSelected(data.find((p) => p.id === selected.id) ?? null);
      }
      return data;
    } catch (e) {
      message.error(`扫描插件失败：${e}`);
      return [];
    } finally {
      setLoading(false);
    }
  }

  async function handleInstall() {
    try {
      const picked = await openDialog({
        directory: false,
        multiple: false,
        title: "选择 Firstwork 插件包",
        filters: [{ name: "Firstwork 插件", extensions: ["firstwork-plugin"] }],
      });
      if (!picked || Array.isArray(picked)) return;
      setInstalling(true);
      const inspection = await pluginApi.inspectArchive(picked);
      await confirmArchiveInstall(inspection);
      await scanPlugins();
    } catch (e) {
      if (String(e).includes("cancelled")) return;
      message.error(`安装失败：${e}`);
    } finally {
      setInstalling(false);
    }
  }

  async function handleInstallDevDirectory() {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: "选择开发插件目录",
      });
      if (!picked || Array.isArray(picked)) return;
      setInstalling(true);
      await pluginApi.installFromDir(picked);
      message.success("开发目录插件已安装");
      await scanPlugins();
    } catch (e) {
      message.error(`安装失败：${e}`);
    } finally {
      setInstalling(false);
    }
  }

  function confirmArchiveInstall(inspection: PluginArchiveInspection) {
    const manifest = inspection.manifest;
    const canInstall = inspection.compatibility.compatible
      && inspection.conflicts.length === 0
      && inspection.missingDependencies.length === 0
      && inspection.runtimePolicy.canExecute;

    return new Promise<void>((resolve, reject) => {
      Modal.confirm({
        width: 720,
        title: `${manifest.name} ${manifest.version}`,
        okText: "确认安装",
        cancelText: "取消",
        okButtonProps: { disabled: !canInstall },
        content: (
          <Space direction="vertical" size={10} style={{ width: "100%" }}>
            <Descriptions size="small" column={2} bordered>
              <Descriptions.Item label="插件 ID">{manifest.id}</Descriptions.Item>
              <Descriptions.Item label="分类">{manifest.classification}</Descriptions.Item>
              <Descriptions.Item label="运行时">{manifest.runtimeKind}</Descriptions.Item>
              <Descriptions.Item label="适用场景">
                {manifest.supportedScenes.join("、") || "全局"}
              </Descriptions.Item>
              <Descriptions.Item label="文件数">{inspection.fileCount}</Descriptions.Item>
              <Descriptions.Item label="签名">{inspection.signatureStatus}</Descriptions.Item>
              <Descriptions.Item label="SHA-256" span={2}>
                <Text copyable ellipsis style={{ maxWidth: 520 }}>{inspection.contentHash}</Text>
              </Descriptions.Item>
            </Descriptions>
            <div>
              <Text strong>请求权限</Text>
              <div style={{ marginTop: 6 }}>
                <Space wrap size={[0, 6]}>
                  {manifest.permissions.length === 0
                    ? <Tag>无权限</Tag>
                    : manifest.permissions.map((permission) => (
                      <Tag key={permission} color={inspection.addedPermissions.includes(permission) ? "orange" : "blue"}>
                        {permissionLabel(permission)}
                      </Tag>
                    ))}
                </Space>
              </div>
            </div>
            {inspection.removedPermissions.length > 0 && (
              <Alert
                type="info"
                showIcon
                message={`本版本移除权限：${inspection.removedPermissions.map(permissionLabel).join("、")}`}
              />
            )}
            {inspection.signatureStatus === "unsigned" && (
              <Alert type="warning" showIcon message="这是未签名插件；继续即表示你明确接受该风险。" />
            )}
            {!inspection.compatibility.compatible && (
              <Alert type="error" showIcon message={inspection.compatibility.reason || "应用版本不兼容"} />
            )}
            {!inspection.runtimePolicy.canExecute && (
              <Alert type="error" showIcon message={inspection.runtimePolicy.blockedReason || "运行时安全策略不允许执行"} />
            )}
            {inspection.missingDependencies.length > 0 && (
              <Alert type="error" showIcon message={`缺少依赖：${inspection.missingDependencies.join("、")}`} />
            )}
            {inspection.conflicts.length > 0 && (
              <Alert type="error" showIcon message={`存在冲突：${inspection.conflicts.join("、")}`} />
            )}
            {inspection.warnings.map((warning) => (
              <Alert key={warning} type="warning" showIcon message={warning} />
            ))}
          </Space>
        ),
        async onOk() {
          try {
            await pluginApi.installArchive({
              path: inspection.archivePath,
              expectedHash: inspection.contentHash,
              approvedPermissions: manifest.permissions,
              confirmUnsigned: inspection.signatureStatus === "unsigned",
            });
            message.success("插件包已安全安装");
            resolve();
          } catch (error) {
            reject(error);
            throw error;
          }
        },
        onCancel() {
          reject(new Error("cancelled"));
        },
      });
    });
  }

  async function handleToggle(plugin: PluginInfo, enabled: boolean) {
    try {
      if (enabled) {
        const policy = await pluginApi.canExecuteRuntime(plugin.id);
        if (!policy.canExecute) {
          message.error(policy.blockedReason || "插件运行时被安全策略阻止");
          return;
        }
        if (plugin.permissions.length > 0) {
          await new Promise<void>((resolve, reject) => {
            Modal.confirm({
              title: "确认启用插件",
              content: (
                <Space direction="vertical" size={8}>
                  <Text>启用前请确认插件声明的权限：</Text>
                  <Space size={[0, 6]} wrap>
                    {plugin.permissions.map((permission) => (
                      <Tag key={permission}>{permissionLabel(permission)}</Tag>
                    ))}
                  </Space>
                </Space>
              ),
              okText: "启用",
              cancelText: "取消",
              onOk: () => resolve(),
              onCancel: () => reject(new Error("cancelled")),
            });
          });
        }
        await pluginApi.enable(plugin.id);
        // 启用的同时激活插件运行时
        const updated = { ...plugin, enabled: true };
        try {
          await pluginManager.activatePlugin(updated);
        } catch (e) {
          console.error(`[PluginsPage] 插件 ${plugin.id} 运行时激活失败:`, e);
          message.warning(`插件「${plugin.name}」已启用，但运行时启动失败：${e}`);
        }
      } else {
        // 先停用运行时，再写数据库
        try {
          await pluginManager.deactivatePlugin(plugin.id);
        } catch (e) {
          console.warn(`[PluginsPage] 停用插件 ${plugin.id} 运行时失败:`, e);
        }
        await pluginApi.disable(plugin.id);
      }
      setPlugins((prev) =>
        prev.map((p) => (p.id === plugin.id ? { ...p, enabled } : p)),
      );
      if (selected?.id === plugin.id) {
        setSelected({ ...selected, enabled });
      }
      notifyDeclarativePluginToolbarChanged();
    } catch (e) {
      if (String(e).includes("cancelled")) return;
      message.error(`切换失败：${e}`);
    }
  }

  function confirmUninstall(plugin: PluginInfo) {
    Modal.confirm({
      title: `卸载插件「${plugin.name}」？`,
      content: "将删除插件目录与本地设置，操作不可撤销。",
      okText: "卸载",
      okType: "danger",
      cancelText: "取消",
      async onOk() {
        try {
          // 先停用运行时
          try {
            await pluginManager.deactivatePlugin(plugin.id);
          } catch (e) {
            console.warn(`[PluginsPage] 停用插件 ${plugin.id} 运行时失败:`, e);
          }
          await pluginApi.uninstall(plugin.id);
          notifyDeclarativePluginToolbarChanged();
          message.success("已卸载");
          if (selected?.id === plugin.id) setSelected(null);
          await loadPlugins();
        } catch (e) {
          message.error(`卸载失败：${e}`);
          throw e;
        }
      },
    });
  }

  async function grantAll(plugin: PluginInfo) {
    const pending = plugin.permissions.filter(
      (p) => !plugin.grantedPermissions.includes(p),
    );
    if (pending.length === 0) return;
    try {
      await pluginApi.grantPermissions(plugin.id, pending);
      notifyDeclarativePluginToolbarChanged();
      message.success("已授权");
      await scanPlugins();
    } catch (e) {
      message.error(`授权失败：${e}`);
    }
  }

  async function revokeAll(plugin: PluginInfo) {
    if (plugin.grantedPermissions.length === 0) return;
    try {
      await pluginApi.revokePermissions(plugin.id, plugin.grantedPermissions);
      notifyDeclarativePluginToolbarChanged();
      message.success("已撤销授权");
      await scanPlugins();
    } catch (e) {
      message.error(`撤销失败：${e}`);
    }
  }

  const columns: ColumnsType<PluginInfo> = useMemo(
    () => [
      {
        title: "插件",
        dataIndex: "name",
        width: 280,
        render: (_, record) => (
          <div style={{ minWidth: 240, maxWidth: 320 }}>
            <Space size={6} align="center" wrap={false}>
              <Tooltip title={record.name}>
                <Text
                  strong
                  ellipsis
                  style={{
                    display: "inline-block",
                    maxWidth: 210,
                    whiteSpace: "nowrap",
                  }}
                >
                  {record.name}
                </Text>
              </Tooltip>
              <Tag color="geekblue">v{record.version}</Tag>
            </Space>
            <Text
              type="secondary"
              style={{
                display: "block",
                fontSize: 12,
                lineHeight: 1.5,
                marginTop: 4,
                whiteSpace: "normal",
                wordBreak: "break-word",
              }}
            >
              {record.description || record.id}
            </Text>
          </div>
        ),
      },
      {
        title: "作者",
        dataIndex: "author",
        width: 140,
        render: (author) => author || <Text type="secondary">未知</Text>,
      },
      {
        title: "状态",
        dataIndex: "status",
        width: 110,
        render: (status) => <Tag color={statusColor(status)}>{status}</Tag>,
      },
      {
        title: "权限",
        key: "runtime",
        dataIndex: "runtimeKind",
        width: 170,
        render: (_, record) => (
          <Space direction="vertical" size={2}>
            <Space size={4} wrap>
              <Tag color={record.runtimeKind === "legacy-js" ? "orange" : "blue"}>
                {record.runtimeKind}
              </Tag>
              {record.manifestFormat === "legacy" && <Tag>兼容模式</Tag>}
            </Space>
            <Text type="secondary" style={{ fontSize: 11 }}>
              {record.source}
            </Text>
          </Space>
        ),
      },
      {
        title: "安全",
        width: 180,
        render: (_, record) => (
          <Space direction="vertical" size={2}>
            <Space size={4} wrap>
              <Tag color={signatureColor(record.signatureStatus)}>
                {record.signatureStatus}
              </Tag>
              <Tag color={record.integrityStatus === "installed" ? "green" : "default"}>
                hash {record.integrityStatus}
              </Tag>
            </Space>
            {!record.canExecute && (
              <Tooltip title={record.blockedReason || "blocked"}>
                <Tag color="red">不可执行</Tag>
              </Tooltip>
            )}
          </Space>
        ),
      },
      {
        key: "runtime",
        dataIndex: "permissions",
        render: (_, record) => {
          if (record.permissions.length === 0) {
            return <Tag>无权限</Tag>;
          }
          return (
            <Space size={[0, 4]} wrap>
              {record.permissions.map((permission) => {
                const granted = record.grantedPermissions.includes(permission);
                return (
                  <Tooltip key={permission} title={permissionDescription(permission)}>
                    <Tag color={granted ? "green" : "orange"}>
                      {permissionLabel(permission)}
                    </Tag>
                  </Tooltip>
                );
              })}
            </Space>
          );
        },
      },
      {
        title: "启用",
        dataIndex: "enabled",
        width: 90,
        render: (_, record) => (
          <Switch
            checked={record.enabled}
            disabled={record.status !== "installed" || !record.canExecute}
            onChange={(checked) => handleToggle(record, checked)}
          />
        ),
      },
      {
        title: "操作",
        width: 220,
        render: (_, record) => (
          <Space>
            <Button size="small" onClick={() => setSelected(record)}>
              详情
            </Button>
            {record.enabled && pluginManager.isActive(record.id) && (
              <Tooltip title="重新加载插件">
                <Button
                  size="small"
                  icon={<RotateCw size={14} />}
                  loading={false}
                  onClick={async () => {
                    try {
                      await pluginManager.reloadPlugin(record.id);
                      message.success("插件已重新加载");
                    } catch (e) {
                      message.error(`重载失败：${e}`);
                    }
                  }}
                />
              </Tooltip>
            )}
            <Button
              size="small"
              danger
              icon={<Trash2 size={14} />}
              onClick={() => confirmUninstall(record)}
            >
              卸载
            </Button>
          </Space>
        ),
      },
    ],
    [selected],
  );

  return (
    <div className="max-w-6xl mx-auto">
      <Card
        title={
          <span className="flex items-center gap-2">
            <Plug size={18} />
            插件
          </span>
        }
        extra={
          <Space>
            <Button icon={<RefreshCw size={14} />} onClick={scanPlugins}>
              扫描
            </Button>
            <Button loading={installing} onClick={handleInstallDevDirectory}>
              开发目录
            </Button>
            <Button
              type="primary"
              icon={<FolderPlus size={14} />}
              loading={installing}
              onClick={handleInstall}
            >
              安装插件包
            </Button>
          </Space>
        }
      >
        <Alert
          type="info"
          showIcon
          style={{ marginBottom: 16 }}
          title="正式插件包采用两阶段安全安装"
          description="先预检 Manifest、兼容性、依赖冲突、权限差异、签名、敏感信息和压缩包安全，再经确认原子安装；开发目录入口仅用于本地调试。"
        />

        <Table
          rowKey="id"
          loading={loading}
          columns={columns}
          dataSource={plugins}
          scroll={{ x: 1280 }}
          locale={{
            emptyText: (
              <Empty
                description="暂无插件"
                image={Empty.PRESENTED_IMAGE_SIMPLE}
              />
            ),
          }}
          pagination={false}
        />

        {/* 错误日志（最近 100 条，按插件分组） */}
        <PluginErrorLog />
      </Card>

      <Modal
        open={!!selected}
        title={selected ? `插件详情：${selected.name}` : "插件详情"}
        width={760}
        footer={
          selected ? (
            <Space>
              <Button onClick={() => setSelected(null)}>关闭</Button>
              <Button
                icon={<ShieldOff size={14} />}
                disabled={selected.grantedPermissions.length === 0}
                onClick={() => revokeAll(selected)}
              >
                撤销全部授权
              </Button>
              <Button
                type="primary"
                icon={<ShieldCheck size={14} />}
                disabled={selected.permissions.every((p) =>
                  selected.grantedPermissions.includes(p),
                )}
                onClick={() => grantAll(selected)}
              >
                授权全部权限
              </Button>
            </Space>
          ) : null
        }
        onCancel={() => setSelected(null)}
      >
        {selected && (
          <Space orientation="vertical" style={{ width: "100%" }} size="middle">
            <Descriptions column={1} bordered size="small">
              <Descriptions.Item label="ID">{selected.id}</Descriptions.Item>
              <Descriptions.Item label="版本">{selected.version}</Descriptions.Item>
              <Descriptions.Item label="作者">
                {selected.author || "未知"}
              </Descriptions.Item>
              <Descriptions.Item label="入口文件">{selected.main}</Descriptions.Item>
              <Descriptions.Item label="样式文件">
                {selected.styles || "无"}
              </Descriptions.Item>
              <Descriptions.Item label="最低应用版本">
                {selected.minAppVersion || "未声明"}
              </Descriptions.Item>
              <Descriptions.Item label="安装路径">
                <Paragraph copyable style={{ marginBottom: 0 }}>
                  {selected.path}
                </Paragraph>
              </Descriptions.Item>
            </Descriptions>

            <div>
              <Text strong>权限</Text>
              <div style={{ marginTop: 8 }}>
                {selected.permissions.length === 0 ? (
                  <Tag>无权限</Tag>
                ) : (
                  <Space size={[0, 6]} wrap>
                    {selected.permissions.map((permission) => (
                      <Tooltip key={permission} title={permissionDescription(permission)}>
                        <Tag
                          color={
                            selected.grantedPermissions.includes(permission)
                              ? "green"
                              : "orange"
                          }
                        >
                          {permissionLabel(permission)}
                        </Tag>
                      </Tooltip>
                    ))}
                  </Space>
                )}
              </div>
            </div>

            <div>
              <Text strong>贡献点</Text>
              <div style={{ marginTop: 8 }}>
                <Space size={[0, 6]} wrap>
                  <Tag>命令 {selected.manifest.contributes.commands.length}</Tag>
                  <Tag>侧栏视图 {selected.manifest.contributes.sidebarViews.length}</Tag>
                  {selected.manifest.contributes.settings && <Tag>设置面板</Tag>}
                </Space>
              </div>
            </div>

            {/* 审计日志（T25） */}
            <div>
              <Text strong>审计日志（最近 20 条）</Text>
              <PluginAuditLog pluginId={selected.id} />
            </div>
            <PluginVersionActivationPanel
              pluginId={selected.id}
              onChanged={scanPlugins}
            />
            <PluginFeatureLinks pluginId={selected.id} />
            {selected.id === "official-ai-document-summary-plugin" && (
              <DocumentSummaryPluginSettings pluginId={selected.id} />
            )}
          </Space>
        )}
      </Modal>
    </div>
  );
}
