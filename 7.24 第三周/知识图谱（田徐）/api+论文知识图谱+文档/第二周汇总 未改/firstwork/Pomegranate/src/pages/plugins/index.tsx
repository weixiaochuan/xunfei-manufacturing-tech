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
import type { PluginInfo, PluginAuditLogEntry, PluginDocumentSummaryConfig } from "@/types";
import { pluginManager } from "@/services/pluginManager";
import { notifyDeclarativePluginToolbarChanged } from "@/services/declarativePluginEvents";

const { Text, Paragraph } = Typography;

const PERMISSION_LABELS: Record<string, string> = {
  "document.read": "读取当前文档",
  "document.write": "写入当前文档",
  "ui.editor.toolbar": "注册编辑器工具栏按钮",
  "editor:read": "读取编辑器",
  "editor:write": "修改编辑器",
  "workspace:read": "读取工作区",
  "workspace:write": "修改工作区",
  "notes:read": "读取笔记",
  "notes:write": "修改笔记",
  "settings:read": "读取设置",
  "settings:write": "修改设置",
  "files:read": "读取文件",
  "files:write": "写入文件",
  "network:request": "网络请求",
  "clipboard:read": "读取剪贴板",
  "clipboard:write": "写入剪贴板",
  "notes.read": "读取笔记",
  "notes.write": "修改笔记",
  "tasks.read": "读取待办",
  "tasks.write": "修改待办",
  "ai.invoke": "调用 AI",
  "network.request": "受控网络请求",
  "files.readSelected": "读取用户选择文件",
  "files.writeSelected": "写入用户选择位置",
  "prompts.register": "注册 Prompt",
  "views.register": "注册视图",
  "mcp.connect": "连接 MCP",
  "credentials.use": "使用凭据 ID",
};

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
  return PERMISSION_LABELS[permission] ?? permission;
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
        directory: true,
        multiple: false,
        title: "选择插件目录",
      });
      if (!picked || Array.isArray(picked)) return;
      setInstalling(true);
      await pluginApi.installFromDir(picked);
      message.success("插件已安装");
      await scanPlugins();
    } catch (e) {
      message.error(`安装失败：${e}`);
    } finally {
      setInstalling(false);
    }
  }

  async function handleToggle(plugin: PluginInfo, enabled: boolean) {
    try {
      if (enabled) {
        const policy = await pluginApi.canExecuteRuntime(plugin.id);
        if (!policy.canExecute) {
          message.error(policy.blockedReason || "æ’ä»¶è¿è¡Œæ—¶è¢«å®‰å…¨ç­–ç•¥é˜»æ­¢");
          return;
        }
        if (plugin.permissions.length > 0) {
          await new Promise<void>((resolve, reject) => {
            Modal.confirm({
              title: "ç¡®è®¤å¯ç”¨æ’ä»¶",
              content: (
                <Space direction="vertical" size={8}>
                  <Text>å¯ç”¨å‰è¯·ç¡®è®¤æ’ä»¶å£°æ˜Žçš„æƒé™ï¼š</Text>
                  <Space size={[0, 6]} wrap>
                    {plugin.permissions.map((permission) => (
                      <Tag key={permission}>{permissionLabel(permission)}</Tag>
                    ))}
                  </Space>
                </Space>
              ),
              okText: "å¯ç”¨",
              cancelText: "å–æ¶ˆ",
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
        render: (_, record) => (
          <Space orientation="vertical" size={0}>
            <Space size={6}>
              <Text strong>{record.name}</Text>
              <Tag color="geekblue">v{record.version}</Tag>
            </Space>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {record.description || record.id}
            </Text>
          </Space>
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
                  <Tooltip key={permission} title={permission}>
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
            <Button
              type="primary"
              icon={<FolderPlus size={14} />}
              loading={installing}
              onClick={handleInstall}
            >
              安装本地插件
            </Button>
          </Space>
        }
      >
        <Alert
          type="info"
          showIcon
          style={{ marginBottom: 16 }}
          title="当前是插件系统 MVP"
          description="已支持插件运行时加载、侧边栏 UI 注入、面板视图、权限授权与设置存储。"
        />

        <Table
          rowKey="id"
          loading={loading}
          columns={columns}
          dataSource={plugins}
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
                      <Tag
                        key={permission}
                        color={
                          selected.grantedPermissions.includes(permission)
                            ? "green"
                            : "orange"
                        }
                      >
                        {permissionLabel(permission)}
                      </Tag>
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
            {selected.id === "official-ai-document-summary-plugin" && (
              <DocumentSummaryPluginSettings pluginId={selected.id} />
            )}
          </Space>
        )}
      </Modal>
    </div>
  );
}
