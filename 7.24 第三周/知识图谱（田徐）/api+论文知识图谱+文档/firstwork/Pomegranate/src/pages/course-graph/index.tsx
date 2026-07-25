import { useEffect, useMemo, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Empty,
  Input,
  List,
  Segmented,
  Space,
  Spin,
  Statistic,
  Tag,
  Typography,
  message,
  theme as antdTheme,
} from "antd";
import cytoscape, { type Core, type ElementDefinition } from "cytoscape";
import {
  BrainCircuit,
  BookOpen,
  Check,
  Crosshair,
  Database,
  Maximize2,
  Minimize2,
  Network,
  RefreshCcw,
  Search,
  X,
} from "lucide-react";

import { courseGraphApi } from "@/lib/api";
import type {
  CourseGraphAiAnalysis,
  CourseGraphAiRelation,
  CourseGraphConfig,
  CourseGraphHealth,
  CourseGraphStats,
} from "@/types";

type CourseNodeType = "Chapter" | "Section" | "Knowledge" | "Concept" | "Entity";

interface RawCourseNode {
  elementId?: string;
  id?: string;
  name?: string;
  content?: string;
  labels?: string[];
  nodeType?: string;
  chapterId?: string | null;
  sectionId?: string | null;
  metadata?: Record<string, unknown>;
  [key: string]: unknown;
}

interface RawCourseEdge {
  elementId?: string;
  id?: string;
  startNodeElementId?: string;
  endNodeElementId?: string;
  source?: string;
  target?: string;
  type?: string;
  metadata?: Record<string, unknown>;
  [key: string]: unknown;
}

interface CourseGraphNode {
  id: string;
  businessId: string;
  label: string;
  type: CourseNodeType;
  raw: RawCourseNode;
}

interface CourseGraphEdge {
  id: string;
  source: string;
  target: string;
  label: string;
  raw: RawCourseEdge;
}

interface BranchRecord {
  nodeIds: string[];
  edgeIds: string[];
}

interface GraphPayload {
  nodes: CourseGraphNode[];
  edges: CourseGraphEdge[];
}

const NODE_COLORS: Record<CourseNodeType, string> = {
  Chapter: "#2563eb",
  Section: "#16a34a",
  Knowledge: "#f59e0b",
  Concept: "#94a3b8",
  Entity: "#64748b",
};

const NODE_LABELS: Record<CourseNodeType, string> = {
  Chapter: "章",
  Section: "节",
  Knowledge: "知识点",
  Concept: "概念",
  Entity: "节点",
};

const LAYOUT_OPTIONS = [
  { label: "层级", value: "hierarchy" },
  { label: "力导", value: "cose" },
  { label: "同心", value: "concentric" },
  { label: "网格", value: "grid" },
];

export default function CourseGraphPage() {
  const { token } = antdTheme.useToken();
  const graphContainerRef = useRef<HTMLDivElement>(null);
  const cyRef = useRef<Core | null>(null);

  const [config, setConfig] = useState<CourseGraphConfig | null>(null);
  const [health, setHealth] = useState<CourseGraphHealth | null>(null);
  const [stats, setStats] = useState<CourseGraphStats | null>(null);
  const [loadingHealth, setLoadingHealth] = useState(false);
  const [loadingGraph, setLoadingGraph] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nodes, setNodes] = useState<Map<string, CourseGraphNode>>(new Map());
  const [edges, setEdges] = useState<Map<string, CourseGraphEdge>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [branches, setBranches] = useState<Map<string, BranchRecord>>(new Map());
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [nodeDetail, setNodeDetail] = useState<Record<string, unknown> | null>(null);
  const [aiAnalysis, setAiAnalysis] = useState<CourseGraphAiAnalysis | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [reviewingRelationId, setReviewingRelationId] = useState<number | null>(null);
  const [searchText, setSearchText] = useState("");
  const [layout, setLayout] = useState("hierarchy");

  const graphNodes = useMemo(() => [...nodes.values()], [nodes]);
  const graphEdges = useMemo(() => [...edges.values()], [edges]);
  const selectedNode = selectedNodeId ? nodes.get(selectedNodeId) ?? null : null;
  const ready = health?.reachable === true;

  useEffect(() => {
    void initialize();
    return () => {
      cyRef.current?.destroy();
      cyRef.current = null;
    };
  }, []);

  useEffect(() => {
    if (!graphContainerRef.current || graphNodes.length === 0) {
      cyRef.current?.destroy();
      cyRef.current = null;
      return;
    }

    cyRef.current?.destroy();
    const cy = cytoscape({
      container: graphContainerRef.current,
      elements: toCytoscapeElements(graphNodes, graphEdges, nodes, expanded),
      style: [
        {
          selector: "node",
          style: {
            "background-color": "data(color)",
            "border-color": "#fff",
            "border-width": "2px",
            "label": "data(label)",
            "font-size": "11px",
            "font-weight": 500,
            "color": token.colorText,
            "text-valign": "bottom",
            "text-halign": "center",
            "text-margin-y": 7,
            "text-wrap": "wrap",
            "text-max-width": "104px",
            "text-background-color": token.colorBgContainer,
            "text-background-opacity": 0.82,
            "text-background-padding": "2px",
            "width": "data(size)",
            "height": "data(size)",
          },
        },
        {
          selector: "node:selected",
          style: {
            "border-color": token.colorPrimary,
            "border-width": "5px",
          },
        },
        {
          selector: "node[expanded]",
          style: {
            "border-color": token.colorSuccess,
            "border-width": "4px",
          },
        },
        {
          selector: "edge",
          style: {
            "curve-style": "unbundled-bezier",
            "target-arrow-shape": "triangle",
            "target-arrow-color": "data(color)",
            "line-color": "data(color)",
            "width": "data(width)",
            "opacity": 0.68,
            "label": "",
            "font-size": "8px",
            "color": token.colorTextSecondary,
            "text-background-color": token.colorBgContainer,
            "text-background-opacity": 0.9,
            "text-background-padding": "2px",
          },
        },
        {
          selector: "edge.focused-edge, edge.hover-edge",
          style: {
            "label": "data(label)",
            "opacity": 1,
            "z-index": 10,
          },
        },
        {
          selector: ".dimmed",
          style: { "opacity": 0.16 },
        },
        {
          selector: "node.focus-node",
          style: {
            "border-color": token.colorPrimary,
            "border-width": "5px",
            "z-index": 20,
          },
        },
      ],
      layout: layoutOptions(layout),
      wheelSensitivity: 0.18,
      minZoom: 0.15,
      maxZoom: 3,
    });

    cy.on("tap", "node", (evt) => {
      const nodeId = evt.target.id();
      const node = nodes.get(nodeId);
      cy.elements().removeClass("dimmed focus-node focused-edge");
      cy.elements().addClass("dimmed");
      evt.target.closedNeighborhood().removeClass("dimmed");
      evt.target.addClass("focus-node");
      evt.target.connectedEdges().addClass("focused-edge");
      if (node) void handleSelectNode(node);
    });
    cy.on("dbltap", "node", (evt) => {
      const nodeId = evt.target.id();
      const node = nodes.get(nodeId);
      if (node) void toggleExpand(node);
    });
    cy.on("mouseover", "edge", (evt) => evt.target.addClass("hover-edge"));
    cy.on("mouseout", "edge", (evt) => evt.target.removeClass("hover-edge"));
    cy.on("tap", (evt) => {
      if (evt.target !== cy) return;
      cy.elements().removeClass("dimmed focus-node focused-edge");
    });
    cyRef.current = cy;

    return () => {
      cy.destroy();
      if (cyRef.current === cy) cyRef.current = null;
    };
  }, [graphNodes, graphEdges, layout, expanded, nodes, token]);

  async function initialize() {
    try {
      const nextConfig = await courseGraphApi.getConfig();
      setConfig(nextConfig);
      const nextHealth = await refreshHealth();
      if (nextHealth?.reachable) {
        await loadChapters(false);
      }
    } catch (e) {
      setError(formatError(e));
    }
  }

  async function refreshHealth() {
    setLoadingHealth(true);
    try {
      const nextHealth = await courseGraphApi.health();
      setHealth(nextHealth);
      setStats(nextHealth.stats ?? null);
      setError(nextHealth.reachable ? null : nextHealth.error ?? "课程图谱 SQLite 资源不可用");
      return nextHealth;
    } catch (e) {
      const text = formatError(e);
      setError(text);
      setHealth(null);
      setStats(null);
      return null;
    } finally {
      setLoadingHealth(false);
    }
  }

  async function loadChapters(showToast = true) {
    setLoadingGraph(true);
    setNodeDetail(null);
    try {
      const data = await courseGraphApi.chapters();
      const payload = normalizePayload(data);
      setNodes(new Map(payload.nodes.map((node) => [node.id, node])));
      setEdges(new Map(payload.edges.map((edge) => [edge.id, edge])));
      setExpanded(new Set());
      setBranches(new Map());
      setSelectedNodeId(null);
      setError(null);
      if (showToast) message.success("已加载课程章节");
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoadingGraph(false);
    }
  }

  async function runSearch() {
    const query = searchText.trim();
    if (!query) {
      message.warning("请输入中文关键词");
      return;
    }
    setLoadingGraph(true);
    setNodeDetail(null);
    try {
      const data = await courseGraphApi.search(query, 20);
      const payload = normalizePayload(data);
      setNodes(new Map(payload.nodes.map((node) => [node.id, node])));
      setEdges(new Map(payload.edges.map((edge) => [edge.id, edge])));
      setExpanded(new Set());
      setBranches(new Map());
      setSelectedNodeId(payload.nodes[0]?.id ?? null);
      setError(null);
      message.success(payload.nodes.length ? `找到 ${payload.nodes.length} 个节点` : "未找到结果");
    } catch (e) {
      setError(formatError(e));
    } finally {
      setLoadingGraph(false);
    }
  }

  async function handleSelectNode(node: CourseGraphNode) {
    setSelectedNodeId(node.id);
    setNodeDetail(null);
    setAiAnalysis(null);
    try {
      const [detail, savedAnalysis] = await Promise.all([
        courseGraphApi.nodeDetail(node.businessId),
        node.type === "Knowledge" ? courseGraphApi.getAiAnalysis(node.businessId) : Promise.resolve(null),
      ]);
      if (isRecord(detail)) setNodeDetail(detail);
      setAiAnalysis(savedAnalysis);
    } catch (e) {
      message.warning(formatError(e));
    }
  }

  async function analyzeSelectedNode() {
    if (!selectedNode || selectedNode.type !== "Knowledge") return;
    setAnalyzing(true);
    try {
      const result = await courseGraphApi.analyzeWithAi(selectedNode.businessId);
      setAiAnalysis(result);
      message.success("AI 已完成知识点解释与候选关系分析");
    } catch (e) {
      message.error(formatError(e));
    } finally {
      setAnalyzing(false);
    }
  }

  async function reviewAiRelation(relation: CourseGraphAiRelation, status: "accepted" | "rejected") {
    setReviewingRelationId(relation.id);
    try {
      const updated = await courseGraphApi.reviewAiRelation(relation.id, status);
      setAiAnalysis((current) => current ? {
        ...current,
        relations: current.relations.map((item) => item.id === updated.id ? updated : item),
      } : current);
      if (status === "accepted" && selectedNode) {
        await addBranch(
          `${selectedNode.businessId}:AI_ACCEPTED`,
          () => courseGraphApi.acceptedAiGraph(selectedNode.businessId),
        );
      }
      message.success(status === "accepted" ? "已接受 AI 关系" : "已拒绝 AI 关系");
    } catch (e) {
      message.error(formatError(e));
    } finally {
      setReviewingRelationId(null);
    }
  }

  async function toggleExpand(node: CourseGraphNode) {
    if (!["Chapter", "Section", "Knowledge"].includes(node.type)) return;
    if (expanded.has(node.businessId)) {
      collapseNode(node.businessId);
      return;
    }
    await addBranch(node.businessId, () => courseGraphApi.expand(node.businessId));
  }

  async function expandRelated(node: CourseGraphNode) {
    await addBranch(`${node.businessId}:RELATED_TO`, () => courseGraphApi.related(node.businessId));
  }

  async function addBranch(branchId: string, loader: () => Promise<unknown>) {
    setLoadingGraph(true);
    try {
      const data = await loader();
      const payload = normalizePayload(data);
      const existingNodeIds = new Set(nodes.keys());
      const existingEdgeIds = new Set(edges.keys());
      const addedNodeIds = payload.nodes
        .map((item) => item.id)
        .filter((id) => !existingNodeIds.has(id));
      const addedEdgeIds = payload.edges
        .map((item) => item.id)
        .filter((id) => !existingEdgeIds.has(id));

      setNodes((prev) => {
        const next = new Map(prev);
        payload.nodes.forEach((item) => next.set(item.id, item));
        return next;
      });
      setEdges((prev) => {
        const next = new Map(prev);
        payload.edges.forEach((item) => next.set(item.id, item));
        return next;
      });
      setBranches((prev) => new Map(prev).set(branchId, { nodeIds: addedNodeIds, edgeIds: addedEdgeIds }));
      setExpanded((prev) => new Set(prev).add(branchId.split(":")[0]));
    } catch (e) {
      message.error(formatError(e));
    } finally {
      setLoadingGraph(false);
    }
  }

  function collapseNode(businessId: string) {
    const branch = branches.get(businessId);
    if (!branch) return;
    const removeNodeIds = new Set<string>();
    const removeEdgeIds = new Set(branch.edgeIds);
    const nextExpanded = new Set(expanded);
    const nextBranches = new Map(branches);

    function collect(id: string) {
      const childBranch = nextBranches.get(id);
      if (childBranch) {
        childBranch.nodeIds.forEach((nodeId) => {
          const child = nodes.get(nodeId);
          if (child) collect(child.businessId);
        });
        childBranch.edgeIds.forEach((edgeId) => removeEdgeIds.add(edgeId));
        nextBranches.delete(id);
        nextExpanded.delete(id);
      }
    }
    branch.nodeIds.forEach((nodeId) => {
      const child = nodes.get(nodeId);
      if (child) collect(child.businessId);
      removeNodeIds.add(nodeId);
    });
    nextBranches.delete(businessId);
    nextExpanded.delete(businessId);

    setNodes((prev) => {
      const next = new Map(prev);
      removeNodeIds.forEach((id) => next.delete(id));
      return next;
    });
    setEdges((prev) => {
      const next = new Map(prev);
      removeEdgeIds.forEach((id) => next.delete(id));
      return next;
    });
    setBranches(nextBranches);
    setExpanded(nextExpanded);
    if (selectedNodeId && removeNodeIds.has(selectedNodeId)) {
      setSelectedNodeId(null);
      setNodeDetail(null);
    }
  }

  function fitView() {
    cyRef.current?.fit(undefined, 36);
  }

  function fitCenter() {
    const cy = cyRef.current;
    if (!cy) return;
    cy.center();
  }

  function rerunLayout() {
    const cy = cyRef.current;
    if (!cy) return;
    cy.layout(layoutOptions(layout)).run();
  }

  return (
    <div className="flex h-full min-h-0 flex-col p-5" style={{ background: token.colorBgLayout }}>
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2">
            <Network size={26} color={token.colorPrimary} />
            <Typography.Title level={3} className="!m-0">
              机械制造工艺知识图谱
            </Typography.Title>
          </div>
          <Typography.Text type="secondary">
            独立课程知识图谱模块，直接读取随应用携带的 SQLite 资源；不依赖 Docker、Neo4j、Java、Python 或 FastAPI，也不影响现有笔记双向链接图。
          </Typography.Text>
        </div>
        <Space wrap>
          <Tag color={ready ? "success" : "error"}>{ready ? "内置数据库可用" : "数据库不可用"}</Tag>
          <Button icon={<RefreshCcw size={15} />} loading={loadingHealth} onClick={() => void refreshHealth()}>
            检测资源
          </Button>
          <Button type="primary" icon={<BookOpen size={15} />} disabled={!ready} onClick={() => void loadChapters()}>
            加载章节
          </Button>
        </Space>
      </div>

      {error && (
        <Alert
          className="mb-4"
          type="warning"
          showIcon
          message="课程知识图谱 SQLite 资源不可用"
          description={
            <div className="space-y-2">
              <div>{error}</div>
              <div>
                预期资源：
                <Typography.Text code copyable>
                  {config?.databasePath ?? "Pomegranate/src-tauri/resources/process_graph.db"}
                </Typography.Text>
              </div>
              <div>
                修复方式：在开发环境运行
                <Typography.Text code>python Pomegranate/src-tauri/scripts/build_process_graph_db.py</Typography.Text>
                后重新启动应用。
              </div>
            </div>
          }
        />
      )}

      <div className="mb-4 grid grid-cols-2 gap-3 md:grid-cols-4">
        <Card size="small">
          <Statistic title="章节 / 小节" value={`${stats?.chapters ?? 0} / ${stats?.sections ?? 0}`} />
        </Card>
        <Card size="small">
          <Statistic title="知识点 / 概念" value={`${stats?.knowledges ?? 0} / ${stats?.concepts ?? 0}`} />
        </Card>
        <Card size="small">
          <Statistic title="有效关系" value={stats?.edges ?? 0} />
        </Card>
        <Card size="small">
          <Statistic title="跳过坏关系" value={stats?.skippedInvalidRelationships ?? 0} />
        </Card>
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[320px_minmax(0,1fr)_320px] gap-4">
        <Card
          size="small"
          title={
            <Space>
              <Database size={16} />
              资源与检索
            </Space>
          }
          className="min-h-0 overflow-hidden"
          styles={{ body: { height: "calc(100% - 44px)", overflow: "auto" } }}
        >
          <div className="mb-3 text-xs leading-6 text-slate-500">
            <div>运行模式：{config?.mode ?? "bundled-sqlite"}</div>
            <div>数据库：{config?.databaseName ?? "process_graph.db"}</div>
            <div>版本：{stats?.version ?? health?.version ?? "未读取"}</div>
            <div>来源：{stats?.sourceZip ?? "课程 ZIP 转换"}</div>
          </div>

          <Space.Compact className="mb-3 w-full">
            <Input
              placeholder="搜索中文名称或正文，例如：定位基准"
              value={searchText}
              onChange={(e) => setSearchText(e.target.value)}
              onPressEnter={() => void runSearch()}
            />
            <Button icon={<Search size={15} />} disabled={!ready} onClick={() => void runSearch()}>
              搜索
            </Button>
          </Space.Compact>

          <List
            size="small"
            bordered
            dataSource={graphNodes.filter((node) => node.type === "Chapter")}
            locale={{ emptyText: "尚未加载章节" }}
            renderItem={(node) => (
              <List.Item
                className="cursor-pointer"
                onClick={() => {
                  void handleSelectNode(node);
                  void toggleExpand(node);
                }}
              >
                <List.Item.Meta
                  title={node.label}
                  description={expanded.has(node.businessId) ? "已展开，双击图中节点可收起" : "点击展开章节"}
                />
              </List.Item>
            )}
          />
        </Card>

        <Card
          size="small"
          className="min-h-0 overflow-hidden"
          title={
            <Space>
              <Network size={16} />
              Cytoscape 课程图谱
              <Typography.Text type="secondary">
                {graphNodes.length} 节点 / {graphEdges.length} 关系
              </Typography.Text>
            </Space>
          }
          extra={
            <Space>
              <Segmented
                size="small"
                value={layout}
                options={LAYOUT_OPTIONS}
                onChange={(value) => setLayout(value as string)}
              />
              <Button size="small" icon={<RefreshCcw size={14} />} onClick={rerunLayout}>
                布局
              </Button>
              <Button size="small" icon={<Maximize2 size={14} />} onClick={fitView}>
                适应
              </Button>
              <Button size="small" icon={<Minimize2 size={14} />} onClick={fitCenter}>
                居中
              </Button>
              <Button size="small" icon={<Crosshair size={14} />} onClick={() => void loadChapters()} disabled={!ready}>
                恢复
              </Button>
            </Space>
          }
          styles={{ body: { height: "calc(100% - 44px)", padding: 0 } }}
        >
          <Spin spinning={loadingGraph}>
            {graphNodes.length === 0 ? (
              <div className="flex h-full min-h-[520px] items-center justify-center">
                <Empty description="请加载章节或搜索课程知识点" />
              </div>
            ) : (
              <div ref={graphContainerRef} style={{ height: "100%", minHeight: 560, background: token.colorBgContainer }} />
            )}
          </Spin>
        </Card>

        <Card
          size="small"
          title="节点详情"
          className="min-h-0 overflow-hidden"
          styles={{ body: { height: "calc(100% - 44px)", overflow: "auto" } }}
        >
          {!selectedNode ? (
            <Empty description="选择节点查看详情" />
          ) : (
            <Space direction="vertical" className="w-full" size="middle">
              <div>
                <Tag color={NODE_COLORS[selectedNode.type]}>{NODE_LABELS[selectedNode.type]}</Tag>
                <Typography.Title level={5} className="!mt-2 !mb-1">
                  {selectedNode.label}
                </Typography.Title>
                <Typography.Text type="secondary">{selectedNode.businessId}</Typography.Text>
              </div>
              <Space wrap>
                {["Chapter", "Section", "Knowledge"].includes(selectedNode.type) && (
                  <Button size="small" onClick={() => void toggleExpand(selectedNode)}>
                    {expanded.has(selectedNode.businessId) ? "收起下级" : "展开下级"}
                  </Button>
                )}
                {selectedNode.type === "Knowledge" && (
                  <>
                    <Button size="small" onClick={() => void expandRelated(selectedNode)}>
                      展开原始关联
                    </Button>
                    <Button
                      size="small"
                      icon={<BrainCircuit size={14} />}
                      loading={analyzing}
                      onClick={() => void analyzeSelectedNode()}
                    >
                      DeepSeek 分析
                    </Button>
                    <Button
                      size="small"
                      disabled={!aiAnalysis?.relations.some((item) => item.status === "accepted")}
                      onClick={() => void addBranch(
                        `${selectedNode.businessId}:AI_ACCEPTED`,
                        () => courseGraphApi.acceptedAiGraph(selectedNode.businessId),
                      )}
                    >
                      展开 AI 关联
                    </Button>
                  </>
                )}
              </Space>
              <DetailBlock node={selectedNode} detail={nodeDetail} />
              {selectedNode.type === "Knowledge" && (
                <AiAnalysisBlock
                  analysis={aiAnalysis}
                  analyzing={analyzing}
                  reviewingRelationId={reviewingRelationId}
                  onAnalyze={() => void analyzeSelectedNode()}
                  onReview={(relation, status) => void reviewAiRelation(relation, status)}
                />
              )}
            </Space>
          )}
        </Card>
      </div>
    </div>
  );
}

function AiAnalysisBlock({
  analysis,
  analyzing,
  reviewingRelationId,
  onAnalyze,
  onReview,
}: {
  analysis: CourseGraphAiAnalysis | null;
  analyzing: boolean;
  reviewingRelationId: number | null;
  onAnalyze: () => void;
  onReview: (relation: CourseGraphAiRelation, status: "accepted" | "rejected") => void;
}) {
  if (!analysis) {
    return (
      <Card size="small" title={<Space><BrainCircuit size={15} />DeepSeek 增强</Space>}>
        <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="尚未分析此知识点">
          <Button type="primary" loading={analyzing} onClick={onAnalyze}>开始 DeepSeek 分析</Button>
        </Empty>
      </Card>
    );
  }
  return (
    <Card
      size="small"
      title={<Space><BrainCircuit size={15} />DeepSeek 增强</Space>}
      extra={<Tag color="purple">来源版本 {analysis.sourceRevision}</Tag>}
    >
      <Typography.Paragraph>{analysis.definition}</Typography.Paragraph>
      <Typography.Text type="secondary">{analysis.summary}</Typography.Text>
      {analysis.aliases.length > 0 && (
        <div className="mt-3">
          <Typography.Text strong>别名：</Typography.Text>
          {analysis.aliases.map((item) => <Tag key={item}>{item}</Tag>)}
        </div>
      )}
      {analysis.prerequisites.length > 0 && (
        <div className="mt-3"><Typography.Text strong>前置知识：</Typography.Text>{analysis.prerequisites.join("、")}</div>
      )}
      {analysis.applications.length > 0 && (
        <div className="mt-3"><Typography.Text strong>典型应用：</Typography.Text>{analysis.applications.join("、")}</div>
      )}
      {analysis.misconceptions.length > 0 && (
        <Alert className="mt-3" type="warning" showIcon message="常见误区" description={analysis.misconceptions.join("；")} />
      )}
      <div className="mt-4">
        <Typography.Text strong>候选关系（需人工审核）</Typography.Text>
        <List
          className="mt-2"
          size="small"
          dataSource={analysis.relations}
          locale={{ emptyText: "AI 未发现证据充分的新关系" }}
          renderItem={(relation) => (
            <List.Item
              actions={relation.status === "pending" ? [
                <Button
                  key="accept"
                  type="text"
                  size="small"
                  icon={<Check size={14} />}
                  loading={reviewingRelationId === relation.id}
                  onClick={() => onReview(relation, "accepted")}
                >接受</Button>,
                <Button
                  key="reject"
                  type="text"
                  danger
                  size="small"
                  icon={<X size={14} />}
                  disabled={reviewingRelationId === relation.id}
                  onClick={() => onReview(relation, "rejected")}
                >拒绝</Button>,
              ] : [
                <Tag key="status" color={relation.status === "accepted" ? "success" : "default"}>
                  {relation.status === "accepted" ? "已接受" : "已拒绝"}
                </Tag>,
              ]}
            >
              <List.Item.Meta
                title={
                  <Space wrap>
                    <Tag color="geekblue">{relation.relationType}</Tag>
                    <span>{relation.targetNodeName}</span>
                    <Tag>{Math.round(relation.confidence * 100)}%</Tag>
                  </Space>
                }
                description={relation.reason}
              />
            </List.Item>
          )}
        />
      </div>
    </Card>
  );
}

function DetailBlock({
  node,
  detail,
}: {
  node: CourseGraphNode;
  detail: Record<string, unknown> | null;
}) {
  const merged = detail ?? {};
  return (
    <div className="space-y-3">
      <InfoRow label="节点类型" value={NODE_LABELS[node.type]} />
      <InfoRow label="业务 ID" value={node.businessId} />
      <InfoRow label="所属章节" value={stringValue(merged.chapter)} />
      <InfoRow label="所属小节" value={stringValue(merged.section)} />
      <InfoRow label="知识类型" value={stringValue(merged.knowledgeType)} />
      <div>
        <Typography.Text strong>正文</Typography.Text>
        <Typography.Paragraph className="mt-2 whitespace-pre-wrap rounded-lg bg-slate-50 p-3">
          {stringValue(merged.content ?? node.raw.content) || "暂无正文"}
        </Typography.Paragraph>
      </div>
    </div>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <Typography.Text type="secondary">{label}：</Typography.Text>
      <Typography.Text>{value || "无"}</Typography.Text>
    </div>
  );
}

function toCytoscapeElements(
  graphNodes: CourseGraphNode[],
  graphEdges: CourseGraphEdge[],
  nodeMap: Map<string, CourseGraphNode>,
  expanded: Set<string>,
): ElementDefinition[] {
  return [
    ...graphNodes.map((node) => ({
      group: "nodes" as const,
      data: {
        id: node.id,
        label: wrapGraphLabel(node.label),
        fullLabel: node.label,
        type: node.type,
        color: NODE_COLORS[node.type] ?? NODE_COLORS.Entity,
        size: nodeSize(node.type),
        expanded: expanded.has(node.businessId),
      },
      classes: "",
    })),
    ...graphEdges
      .filter((edge) => nodeMap.has(edge.source) && nodeMap.has(edge.target))
      .map((edge) => ({
        group: "edges" as const,
        data: {
          id: edge.id,
          source: edge.source,
          target: edge.target,
          label: edge.label,
          color: edge.raw.metadata?.aiGenerated ? "#8b5cf6" : edge.label === "RELATED_TO" ? "#f97316" : "#94a3b8",
          width: edge.raw.metadata?.aiGenerated ? 3 : edge.label === "RELATED_TO" ? 2.4 : 1.4,
        },
      })),
  ];
}

function layoutOptions(layout: string) {
  if (layout === "hierarchy") {
    return {
      name: "breadthfirst",
      fit: true,
      directed: true,
      circle: false,
      grid: true,
      roots: "node[type = 'Chapter']",
      padding: 42,
      spacingFactor: 1.45,
      avoidOverlap: true,
      nodeDimensionsIncludeLabels: true,
      animate: false,
    };
  }
  if (layout === "grid") {
    return { name: "grid", fit: true, padding: 42, avoidOverlap: true, nodeDimensionsIncludeLabels: true };
  }
  if (layout === "concentric") {
    return { name: "concentric", fit: true, padding: 42, minNodeSpacing: 56, avoidOverlap: true };
  }
  return {
    name: "cose",
    fit: true,
    padding: 36,
    animate: false,
    nodeRepulsion: 12000,
    idealEdgeLength: 155,
    edgeElasticity: 120,
    nestingFactor: 1.3,
    nodeDimensionsIncludeLabels: true,
  };
}

function nodeSize(type: CourseNodeType) {
  if (type === "Chapter") return 48;
  if (type === "Section") return 40;
  if (type === "Knowledge") return 34;
  return 24;
}

function normalizePayload(input: unknown): GraphPayload {
  const rawNodes: RawCourseNode[] = [];
  const rawEdges: RawCourseEdge[] = [];
  scanGraphValues(input, rawNodes, rawEdges);
  const nodes = rawNodes.map(toCourseNode).filter(Boolean) as CourseGraphNode[];
  const seenNodes = new Set<string>();
  const uniqueNodes = nodes.filter((node) => {
    if (seenNodes.has(node.id)) return false;
    seenNodes.add(node.id);
    return true;
  });
  const nodeIds = new Set(uniqueNodes.map((node) => node.id));
  const seenEdges = new Set<string>();
  const edges = rawEdges
    .map(toCourseEdge)
    .filter(Boolean)
    .filter((edge): edge is CourseGraphEdge => {
      if (!edge || seenEdges.has(edge.id)) return false;
      if (!nodeIds.has(edge.source) || !nodeIds.has(edge.target)) return false;
      seenEdges.add(edge.id);
      return true;
    });
  return { nodes: uniqueNodes, edges };
}

function scanGraphValues(value: unknown, nodes: RawCourseNode[], edges: RawCourseEdge[]) {
  if (Array.isArray(value)) {
    value.forEach((item) => scanGraphValues(item, nodes, edges));
    return;
  }
  if (!isRecord(value)) return;
  if (Array.isArray(value.labels) && typeof value.elementId === "string") {
    nodes.push(value as RawCourseNode);
    return;
  }
  if (
    typeof value.elementId === "string" &&
    typeof value.startNodeElementId === "string" &&
    typeof value.endNodeElementId === "string"
  ) {
    edges.push(value as RawCourseEdge);
    return;
  }
  Object.values(value).forEach((item) => scanGraphValues(item, nodes, edges));
}

function toCourseNode(raw: RawCourseNode): CourseGraphNode | null {
  if (!raw.elementId) return null;
  const labels = raw.labels ?? [];
  const type = (raw.nodeType ||
    ["Chapter", "Section", "Knowledge", "Concept"].find((item) => labels.includes(item)) ||
    "Entity") as CourseNodeType;
  return {
    id: raw.elementId,
    businessId: String(raw.id ?? raw.elementId),
    label: String(raw.name ?? raw.id ?? type),
    type,
    raw,
  };
}

function toCourseEdge(raw: RawCourseEdge): CourseGraphEdge | null {
  const source = raw.startNodeElementId ?? raw.source;
  const target = raw.endNodeElementId ?? raw.target;
  const id = raw.elementId ?? raw.id;
  if (!id || !source || !target) return null;
  return {
    id,
    source,
    target,
    label: String(raw.type ?? ""),
    raw,
  };
}

function wrapGraphLabel(label: string, maxLength = 18, lineLength = 9) {
  const characters = Array.from(label.replace(/\s+/g, " ").trim());
  const visible = characters.slice(0, maxLength);
  const lines: string[] = [];
  for (let index = 0; index < visible.length; index += lineLength) {
    lines.push(visible.slice(index, index + lineLength).join(""));
  }
  if (characters.length > maxLength && lines.length > 0) {
    lines[lines.length - 1] += "…";
  }
  return lines.join("\n");
}

function formatError(error: unknown) {
  if (error instanceof Error) return error.message;
  return String(error);
}

function isRecord(value: unknown): value is Record<string, any> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}

function stringValue(value: unknown) {
  if (value == null) return "";
  return String(value);
}
