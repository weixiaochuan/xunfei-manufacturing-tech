import { useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Form,
  Input,
  List,
  Popconfirm,
  Row,
  Space,
  Statistic,
  Tag,
  Typography,
  message,
  theme as antdTheme,
} from "antd";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import cytoscape, { type Core } from "cytoscape";
import {
  ArrowUpRight,
  Bot,
  BookOpen,
  CalendarDays,
  Database,
  FileUp,
  Lightbulb,
  Maximize2,
  Microscope,
  Network,
  RotateCcw,
  Search,
  Sparkles,
  Users,
} from "lucide-react";

import { noteApi, researchApi } from "@/lib/api";
import type {
  ResearchAnalysisResult,
  ResearchGraphEdge,
  ResearchGraphNode,
  ResearchPaper,
  ResearchPaperKnowledgeDecision,
  ResearchPaperKnowledgeRecommendation,
  ResearchPaperSearchResult,
} from "@/types";

const { Title, Paragraph, Text } = Typography;

interface ResearchFormValues {
  topic: string;
  keywords?: string;
}

const WORK_TYPE_LABELS: Record<string, string> = {
  "journal-article": "期刊论文",
  "proceedings-article": "会议论文",
  "posted-content": "预印本",
  "book-chapter": "书籍章节",
  dissertation: "学位论文",
  report: "研究报告",
};

const PAPER_DATABASES = [
  { name: "Crossref", coverage: "跨学科出版物", color: "blue" },
  { name: "Semantic Scholar", coverage: "跨学科学术图谱", color: "purple" },
  { name: "arXiv", coverage: "前沿预印本", color: "volcano" },
  { name: "Europe PMC", coverage: "生命科学与医学", color: "cyan" },
  { name: "PubMed", coverage: "生物医学文献", color: "green" },
  { name: "DBLP", coverage: "计算机科学文献", color: "geekblue" },
] as const;

function buildQuery(values: ResearchFormValues): string {
  const keywords = (values.keywords ?? "")
    .split(/[，,；;]/)
    .map((keyword) => keyword.trim())
    .filter(Boolean);
  return [values.topic.trim(), ...keywords].join(" ");
}

function formatAuthors(authors: string[]): string {
  if (authors.length === 0) return "作者信息未收录";
  if (authors.length <= 4) return authors.join("、");
  return `${authors.slice(0, 4).join("、")} 等`;
}

function formatCount(count: number): string {
  return new Intl.NumberFormat("zh-CN").format(count);
}

function workTypeLabel(type: string): string {
  return WORK_TYPE_LABELS[type] ?? (type || "论文");
}

function sourceColor(source: string): string {
  return PAPER_DATABASES.find((database) => database.name === source)?.color ?? "default";
}

function normalizeComparableText(text: string): string {
  return text.toLocaleLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");
}

function paperReadingPrompts(paper: ResearchPaper): string[] {
  const normalizedTitle = normalizeComparableText(paper.title);
  // 防止历史缓存或第三方摘要首句把论文标题原样展示为阅读提示。
  const prompts = paper.highlights.filter(
    (highlight) => normalizeComparableText(highlight) !== normalizedTitle,
  );
  return prompts.length > 0
    ? prompts
    : ["阅读提示：重点核对本文的研究方法、数据来源、评价指标与适用边界。"];
}

const KNOWLEDGE_DECISION_META: Record<
  ResearchPaperKnowledgeDecision,
  { label: string; color: string; alertType: "success" | "warning" | "info" }
> = {
  recommended: { label: "建议加入", color: "green", alertType: "success" },
  consider: { label: "建议先核验", color: "gold", alertType: "warning" },
  not_recommended: { label: "暂不建议加入", color: "default", alertType: "info" },
};

function knowledgeNoteContent(
  paper: ResearchPaper,
  recommendation: ResearchPaperKnowledgeRecommendation,
  query: string,
): string {
  const decision = KNOWLEDGE_DECISION_META[recommendation.decision];
  const sources = paper.sources.map((source) => source.name).join("、") || "未收录";
  const sourceLinks = paper.sources.length > 0
    ? paper.sources.map((source) => `- [${source.name}](${source.url})`).join("\n")
    : `- [打开论文](${paper.url})`;
  const prompts = paperReadingPrompts(paper).map((prompt) => `- ${prompt}`).join("\n");
  const tags = recommendation.suggestedTags.length > 0
    ? recommendation.suggestedTags.map((tag) => `#${tag.replace(/\s+/g, "-")}`).join(" ")
    : "无";

  return `# ${paper.title}

> 由用户在 AI 助研中确认加入知识库。AI 建议仅作为筛选参考。

## 题录信息

- 检索主题：${query}
- 作者：${paper.authors.join("、") || "未收录"}
- 发表时间：${paper.publicationDate ?? paper.publicationYear}
- 出版载体：${paper.venue ?? paper.publisher ?? "未收录"}
- 类型：${workTypeLabel(paper.workType)}
- 引用量：${paper.citedByCount}
- DOI：${paper.doi ?? "未收录"}
- 论文库来源：${sources}
- 原文链接：${paper.url}

## 入库评估建议

- 评估方式：${recommendation.evaluationMode === "ai" ? "默认 AI 模型" : "本地智能规则兜底"}
- 结论：${decision.label}
- 置信度：${Math.round(recommendation.confidence * 100)}%
- 理由：${recommendation.reason}
- 建议标签：${tags}
${recommendation.warning ? `- 提示：${recommendation.warning}` : ""}

## 系统筛选依据

${paper.rankReason}

## 摘要

${paper.abstractText ?? "论文库暂未提供摘要，建议打开原文核验。"}

## 阅读提示

${prompts}

## 来源链接

${sourceLinks}
`;
}

export default function ResearchAssistantPage() {
  const { token } = antdTheme.useToken();
  const [form] = Form.useForm<ResearchFormValues>();
  const [loading, setLoading] = useState(false);
  const [searchResult, setSearchResult] = useState<ResearchPaperSearchResult | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [paperPaths, setPaperPaths] = useState<string[]>([]);
  const [projectContext, setProjectContext] = useState("");
  const [analyzingPapers, setAnalyzingPapers] = useState(false);
  const [analysisResult, setAnalysisResult] = useState<ResearchAnalysisResult | null>(null);
  const [knowledgeRecommendations, setKnowledgeRecommendations] = useState<
    Record<string, ResearchPaperKnowledgeRecommendation>
  >({});
  const [recommendationLoadingIds, setRecommendationLoadingIds] = useState<Set<string>>(new Set());
  const [addingPaperIds, setAddingPaperIds] = useState<Set<string>>(new Set());
  const [addedNoteIds, setAddedNoteIds] = useState<Record<string, number>>({});
  const [declinedPaperIds, setDeclinedPaperIds] = useState<Set<string>>(new Set());

  async function choosePapers() {
    const selected = await openDialog({
      multiple: true,
      filters: [{ name: "PDF 论文", extensions: ["pdf"] }],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    if (paths.length > 5) {
      message.warning("第一版每次最多分析 5 篇论文，已保留前 5 篇");
    }
    setPaperPaths(paths.slice(0, 5));
    setAnalysisResult(null);
  }

  async function analyzeUploadedPapers() {
    if (paperPaths.length < 2) {
      message.warning("请至少选择 2 篇 PDF 论文");
      return;
    }
    if (projectContext.trim().length < 20) {
      message.warning("请至少用 20 个字描述自己的项目方向、方法或困难");
      return;
    }
    setAnalyzingPapers(true);
    try {
      const result = await researchApi.analyzePapers({
        filePaths: paperPaths,
        projectContext,
      });
      setAnalysisResult(result);
      message.success("多论文分析完成");
    } catch (error) {
      message.error(String(error));
    } finally {
      setAnalyzingPapers(false);
    }
  }

  async function handleSearch(values: ResearchFormValues) {
    setLoading(true);
    setErrorMessage(null);
    try {
      const result = await researchApi.searchPapers({
        query: buildQuery(values),
        limit: 12,
      });
      setSearchResult(result);
      setKnowledgeRecommendations({});
      setRecommendationLoadingIds(new Set());
      setAddingPaperIds(new Set());
      setAddedNoteIds({});
      setDeclinedPaperIds(new Set());
      if (result.papers.length > 0) {
        message.success(`已从多个平台精选 ${result.papers.length} 篇近五年论文`);
      } else {
        message.warning("近五年内暂未找到匹配论文，请尝试更换关键词");
      }
    } catch (error) {
      const text = String(error);
      setSearchResult(null);
      setErrorMessage(text);
      message.error(text);
    } finally {
      setLoading(false);
    }
  }

  async function handleOpenPaper(paper: ResearchPaper) {
    try {
      await openUrl(paper.url);
    } catch (error) {
      message.error(`无法打开论文链接：${String(error)}`);
    }
  }

  async function handleOpenSource(url: string) {
    try {
      await openUrl(url);
    } catch (error) {
      message.error(`无法打开检索平台链接：${String(error)}`);
    }
  }

  async function handleRecommendForKnowledgeBase(paper: ResearchPaper) {
    if (!searchResult) return;
    setRecommendationLoadingIds((current) => new Set(current).add(paper.id));
    setDeclinedPaperIds((current) => {
      const next = new Set(current);
      next.delete(paper.id);
      return next;
    });
    try {
      const recommendation = await researchApi.recommendForKnowledgeBase({
        query: searchResult.query,
        paper,
      });
      setKnowledgeRecommendations((current) => ({
        ...current,
        [paper.id]: recommendation,
      }));
    } catch (error) {
      message.error(`AI 入库评估失败：${String(error)}`);
    } finally {
      setRecommendationLoadingIds((current) => {
        const next = new Set(current);
        next.delete(paper.id);
        return next;
      });
    }
  }

  async function handleAddPaperToKnowledgeBase(paper: ResearchPaper) {
    if (!searchResult) return;
    const recommendation = knowledgeRecommendations[paper.id];
    if (!recommendation) {
      message.warning("请先完成 AI 入库评估");
      return;
    }
    setAddingPaperIds((current) => new Set(current).add(paper.id));
    try {
      const note = await noteApi.create({
        title: `论文｜${paper.title}`,
        content: knowledgeNoteContent(paper, recommendation, searchResult.query),
        folder_id: null,
      });
      setAddedNoteIds((current) => ({ ...current, [paper.id]: note.id }));
      setDeclinedPaperIds((current) => {
        const next = new Set(current);
        next.delete(paper.id);
        return next;
      });
      message.success("已按你的决定加入知识库");
    } catch (error) {
      message.error(`加入知识库失败：${String(error)}`);
    } finally {
      setAddingPaperIds((current) => {
        const next = new Set(current);
        next.delete(paper.id);
        return next;
      });
    }
  }

  function handleDeclinePaper(paperId: string) {
    setDeclinedPaperIds((current) => new Set(current).add(paperId));
    message.info("已保留 AI 建议，本次不加入知识库");
  }

  return (
    <div className="mx-auto max-w-[1180px] pb-8">
      <Space align="start" size={12} className="mb-4">
        <Microscope size={30} color={token.colorPrimary} />
        <div>
          <Title level={2} style={{ margin: 0 }}>
            AI 助研
          </Title>
          <Paragraph type="secondary" style={{ margin: "4px 0 0" }}>
            从 6 个已接入论文库汇总近五年前沿论文，快速比较来源、重点内容、作者与引用情况。
          </Paragraph>
        </div>
      </Space>

      <Card
        className="mb-4"
        title={<Space><FileUp size={18} />上传论文并分析当前项目</Space>}
        extra={<Tag color="purple">使用默认 DeepSeek 模型</Tag>}
      >
        <Row gutter={[16, 16]}>
          <Col xs={24} lg={9}>
            <Button icon={<FileUp size={16} />} onClick={() => void choosePapers()}>
              选择 2～5 篇 PDF
            </Button>
            <List
              className="mt-3"
              size="small"
              bordered
              dataSource={paperPaths}
              locale={{ emptyText: "尚未选择论文" }}
              renderItem={(path, index) => (
                <List.Item
                  actions={[
                    <Button
                      key="remove"
                      type="link"
                      danger
                      size="small"
                      onClick={() => setPaperPaths((current) => current.filter((_, itemIndex) => itemIndex !== index))}
                    >移除</Button>,
                  ]}
                >
                  <Text ellipsis={{ tooltip: path }}>{fileNameFromPath(path)}</Text>
                </List.Item>
              )}
            />
          </Col>
          <Col xs={24} lg={15}>
            <Text strong>当前项目背景</Text>
            <Input.TextArea
              className="mt-2"
              value={projectContext}
              onChange={(event) => setProjectContext(event.target.value)}
              rows={6}
              maxLength={6000}
              showCount
              placeholder="说明你的研究问题、当前方法、已有数据、评价指标、困难和不可改变的限制。AI 会据此判断论文中哪些结论可以借鉴。"
            />
            <Button
              className="mt-3"
              type="primary"
              icon={<Sparkles size={17} />}
              loading={analyzingPapers}
              disabled={paperPaths.length < 2 || projectContext.trim().length < 20}
              onClick={() => void analyzeUploadedPapers()}
            >
              分析摘要、关键词、异同并生成项目建议
            </Button>
          </Col>
        </Row>
      </Card>

      {analysisResult && <ResearchAnalysisView result={analysisResult} />}

      <Card className="mb-4">
        <Form
          form={form}
          layout="vertical"
          initialValues={{ topic: "", keywords: "" }}
          onFinish={handleSearch}
        >
          <Row gutter={16} align="bottom">
            <Col xs={24} lg={13}>
              <Form.Item
                name="topic"
                label="研究主题"
                rules={[
                  { required: true, message: "请输入研究主题" },
                  { max: 200, message: "研究主题请控制在 200 个字符以内" },
                ]}
              >
                <Input
                  size="large"
                  prefix={<Search size={17} color={token.colorTextTertiary} />}
                  placeholder="例如：先进制造、设备故障诊断、机器人视觉伺服"
                />
              </Form.Item>
            </Col>
            <Col xs={24} lg={7}>
              <Form.Item
                name="keywords"
                label="补充关键词（可选）"
                tooltip="多个关键词用逗号分隔，中英文都可以"
              >
                <Input size="large" placeholder="机械制造，智能制造，deep learning" />
              </Form.Item>
            </Col>
            <Col xs={24} lg={4}>
              <Form.Item>
                <Button
                  type="primary"
                  htmlType="submit"
                  size="large"
                  block
                  loading={loading}
                  icon={<Sparkles size={17} />}
                >
                  检索论文
                </Button>
              </Form.Item>
            </Col>
          </Row>
        </Form>
        <Alert
          type="info"
          showIcon
          message={
            <Space wrap>
              <Text strong>已接入论文库</Text>
              <Tag color="green">{PAPER_DATABASES.length} 个</Tag>
            </Space>
          }
          description={
            <Space direction="vertical" size={8}>
              <Text type="secondary">
                一次检索覆盖跨学科、生命科学、医学和计算机领域，并按 DOI 或规范化标题合并重复论文。
              </Text>
              <Space wrap size={[6, 6]}>
                {PAPER_DATABASES.map((database) => (
                  <Tag key={database.name} color={database.color}>
                    {database.name} · {database.coverage}
                  </Tag>
                ))}
              </Space>
              <Text type="secondary">
                无需额外配置密钥；单个论文库暂时不可用时，会自动使用其余来源继续检索。
              </Text>
            </Space>
          }
        />
      </Card>

      {errorMessage ? (
        <Alert
          className="mb-4"
          type="error"
          showIcon
          message="论文检索失败"
          description={errorMessage}
          action={<Button onClick={() => form.submit()}>重新检索</Button>}
        />
      ) : null}

      {!searchResult && !loading ? (
        <Card>
          <Empty
            image={<BookOpen size={64} color={token.colorTextQuaternary} />}
            description="输入研究主题后，这里会显示对应的近五年前沿论文"
          />
        </Card>
      ) : null}

      {loading ? (
        <Card>
          <List
            dataSource={[1, 2, 3, 4]}
            renderItem={(item) => (
              <List.Item key={item}>
                <List.Item.Meta
                  title={<div className="h-5 rounded bg-black/5 dark:bg-white/10 animate-pulse" />}
                  description={<div className="mt-3 h-12 rounded bg-black/5 dark:bg-white/10 animate-pulse" />}
                />
              </List.Item>
            )}
          />
        </Card>
      ) : null}

      {searchResult ? (
        <Space direction="vertical" size={16} style={{ width: "100%" }}>
          {searchResult.warnings.map((warning) => (
            <Alert key={warning} type="warning" showIcon message={warning} />
          ))}
          <Card>
            <Row gutter={[16, 16]} align="middle">
              <Col xs={12} sm={6}>
                <Statistic title="检索年份" value={`${searchResult.fromYear}-${searchResult.toYear}`} />
              </Col>
              <Col xs={12} sm={6}>
                <Statistic title="跨平台匹配" value={formatCount(searchResult.totalResults)} />
              </Col>
              <Col xs={12} sm={6}>
                <Statistic title="精选展示" value={searchResult.papers.length} suffix="篇" />
              </Col>
              <Col xs={12} sm={6}>
                <Statistic
                  title="可用论文库"
                  value={searchResult.sources.filter((source) => source.available).length}
                  suffix={`/${searchResult.sources.length}`}
                />
              </Col>
            </Row>
            <Space wrap size={[6, 6]} className="mt-4">
              <Text type="secondary">论文库状态：</Text>
              {searchResult.sources.map((source) => (
                <Tag
                  key={source.name}
                  color={source.available ? sourceColor(source.name) : "default"}
                >
                  {source.name} · {source.available ? `返回 ${source.resultCount} 篇` : "暂不可用"}
                </Tag>
              ))}
            </Space>
          </Card>

          {searchResult.papers.length === 0 ? (
            <Card>
              <Empty description="没有找到匹配论文，请缩短主题或换一组关键词再试" />
            </Card>
          ) : (
            <Card
              title="跨平台近五年前沿论文"
              extra={<Text type="secondary">综合相关度、年份、引用量与多平台收录情况排序</Text>}
            >
              <List
                itemLayout="vertical"
                dataSource={searchResult.papers}
                renderItem={(paper, index) => (
                  <List.Item
                    key={paper.id}
                    actions={[
                      <Space key="year" size={5}>
                        <CalendarDays size={15} />
                        <span>{paper.publicationDate ?? paper.publicationYear}</span>
                      </Space>,
                      <Space key="authors" size={5}>
                        <Users size={15} />
                        <span>{formatAuthors(paper.authors)}</span>
                      </Space>,
                      <Button
                        key="open"
                        type="link"
                        icon={<ArrowUpRight size={15} />}
                        onClick={() => void handleOpenPaper(paper)}
                      >
                        打开论文
                      </Button>,
                    ]}
                  >
                    <List.Item.Meta
                      title={
                        <Space direction="vertical" size={6} style={{ width: "100%" }}>
                          <Space wrap>
                            <Tag color="blue">#{index + 1}</Tag>
                            <Tag color="geekblue">{workTypeLabel(paper.workType)}</Tag>
                            <Tag color="green">前沿分 {paper.frontierScore}</Tag>
                          </Space>
                          <Text strong style={{ fontSize: 16 }}>
                            {paper.title}
                          </Text>
                        </Space>
                      }
                      description={
                        <Space direction="vertical" size={6}>
                          <Text type="secondary">
                            {paper.venue ?? paper.publisher ?? "来源未收录"}
                            {paper.doi ? ` · DOI: ${paper.doi}` : ""}
                          </Text>
                          <Space wrap>
                            <Text type="secondary" style={{ fontSize: 12 }}>论文库来源：</Text>
                            {paper.sources.map((source) => (
                              <Tag
                                key={source.name}
                                color={sourceColor(source.name)}
                                style={{ cursor: "pointer" }}
                                onClick={() => void handleOpenSource(source.url)}
                              >
                                {source.name}
                              </Tag>
                            ))}
                          </Space>
                          <Space wrap>
                            <Tag>引用 {formatCount(paper.citedByCount)}</Tag>
                          </Space>
                          <div
                            className="rounded-md px-2 py-1"
                            style={{ background: token.colorFillAlter }}
                          >
                            <Text strong style={{ fontSize: 12 }}>筛选依据：</Text>
                            <Text type="secondary" style={{ fontSize: 12, lineHeight: 1.65 }}>
                              {paper.rankReason}
                            </Text>
                          </div>
                        </Space>
                      }
                    />
                    <div
                      className="mt-2 rounded-lg px-3 py-2"
                      style={{ background: token.colorFillAlter }}
                    >
                      <Space align="start" size={6}>
                        <Lightbulb size={14} color={token.colorWarning} style={{ marginTop: 3 }} />
                        <div>
                          <Text strong style={{ fontSize: 12 }}>阅读提示</Text>
                          {paperReadingPrompts(paper).map((prompt, promptIndex) => (
                            <Text
                              key={`${paper.id}-prompt-${promptIndex}`}
                              type="secondary"
                              style={{ display: "block", fontSize: 12, lineHeight: 1.65 }}
                            >
                              · {prompt}
                            </Text>
                          ))}
                        </div>
                      </Space>
                    </div>
                    <div className="mt-3">
                      {addedNoteIds[paper.id] ? (
                        <Alert
                          type="success"
                          showIcon
                          message="已加入知识库"
                          description="论文题录、摘要、阅读提示、筛选依据和 AI 建议已保存为知识库笔记，可参与后续检索。"
                        />
                      ) : declinedPaperIds.has(paper.id) ? (
                        <Alert
                          type="info"
                          showIcon
                          message="本次暂不加入"
                          description={
                            <Button
                              type="link"
                              style={{ padding: 0 }}
                              onClick={() => {
                                setDeclinedPaperIds((current) => {
                                  const next = new Set(current);
                                  next.delete(paper.id);
                                  return next;
                                });
                              }}
                            >
                              重新查看 AI 建议并决定
                            </Button>
                          }
                        />
                      ) : knowledgeRecommendations[paper.id] ? (
                        <Alert
                          type={KNOWLEDGE_DECISION_META[
                            knowledgeRecommendations[paper.id].decision
                          ].alertType}
                          showIcon
                          message={
                            <Space wrap>
                              <Bot size={16} />
                              <Text strong>
                                {knowledgeRecommendations[paper.id].evaluationMode === "ai"
                                  ? "AI "
                                  : "本地智能"}
                                {
                                  KNOWLEDGE_DECISION_META[
                                    knowledgeRecommendations[paper.id].decision
                                  ].label
                                }
                              </Text>
                              <Tag
                                color={
                                  KNOWLEDGE_DECISION_META[
                                    knowledgeRecommendations[paper.id].decision
                                  ].color
                                }
                              >
                                置信度{" "}
                                {Math.round(knowledgeRecommendations[paper.id].confidence * 100)}%
                              </Tag>
                              {knowledgeRecommendations[paper.id].evaluationMode ===
                              "local_fallback" ? (
                                <Tag color="orange">AI 模型不可用，已自动兜底</Tag>
                              ) : null}
                            </Space>
                          }
                          description={
                            <Space direction="vertical" size={8} style={{ width: "100%" }}>
                              <Text
                                style={{
                                  display: "block",
                                  lineHeight: 1.75,
                                  whiteSpace: "pre-line",
                                }}
                              >
                                {knowledgeRecommendations[paper.id].reason}
                              </Text>
                              {knowledgeRecommendations[paper.id].warning ? (
                                <Text type="warning">
                                  {knowledgeRecommendations[paper.id].warning}
                                </Text>
                              ) : null}
                              {knowledgeRecommendations[paper.id].suggestedTags.length > 0 ? (
                                <Space wrap size={[4, 4]}>
                                  <Text type="secondary">建议标签：</Text>
                                  {knowledgeRecommendations[paper.id].suggestedTags.map((tag) => (
                                    <Tag key={tag}>{tag}</Tag>
                                  ))}
                                </Space>
                              ) : null}
                              <Text type="secondary">
                                评估只提供建议，不会自动加入。最终决定由你确认。
                              </Text>
                              <Space wrap>
                                <Popconfirm
                                  title="确认加入知识库？"
                                  description="确认后会创建一篇知识库笔记，AI 建议不会替你自动执行。"
                                  okText="确认加入"
                                  cancelText="取消"
                                  onConfirm={() => handleAddPaperToKnowledgeBase(paper)}
                                >
                                  <Button
                                    type="primary"
                                    icon={<Database size={15} />}
                                    loading={addingPaperIds.has(paper.id)}
                                  >
                                    加入知识库
                                  </Button>
                                </Popconfirm>
                                <Button onClick={() => handleDeclinePaper(paper.id)}>
                                  暂不加入
                                </Button>
                                <Button
                                  type="link"
                                  loading={recommendationLoadingIds.has(paper.id)}
                                  onClick={() => void handleRecommendForKnowledgeBase(paper)}
                                >
                                  重新评估
                                </Button>
                              </Space>
                            </Space>
                          }
                        />
                      ) : (
                        <Alert
                          type="info"
                          showIcon
                          icon={<Bot size={18} />}
                          message="让 AI 评估是否值得加入知识库"
                          description={
                            <Space direction="vertical" size={8}>
                              <Text type="secondary">
                                优先调用默认 AI 模型；模型未配置或暂时不可用时，会自动改用本地规则评估，不影响你决定是否加入。
                              </Text>
                              <Button
                                icon={<Sparkles size={15} />}
                                loading={recommendationLoadingIds.has(paper.id)}
                                onClick={() => void handleRecommendForKnowledgeBase(paper)}
                              >
                                AI 评估是否入库
                              </Button>
                            </Space>
                          }
                        />
                      )}
                    </div>
                  </List.Item>
                )}
              />
            </Card>
          )}
        </Space>
      ) : null}
    </div>
  );
}

function ResearchAnalysisView({ result }: { result: ResearchAnalysisResult }) {
  return (
    <Space direction="vertical" size={16} className="mb-4 w-full">
      {result.warnings.map((warning) => (
        <Alert key={warning} type="warning" showIcon message={warning} />
      ))}
      <Card title="AI 对当前项目的理解">
        <Paragraph>{result.projectSummary}</Paragraph>
      </Card>
      <Card title="逐篇论文分析">
        <List
          itemLayout="vertical"
          dataSource={result.papers}
          renderItem={(paper) => (
            <List.Item key={paper.paperId}>
              <List.Item.Meta
                title={<Space wrap><Tag color="blue">{paper.paperId}</Tag><Text strong>{paper.title || paper.fileName}</Text></Space>}
                description={paper.researchQuestion}
              />
              <Paragraph style={{ marginBottom: 10 }}>
                <Text strong>摘要：</Text>{paper.abstractText || "未提取"}
              </Paragraph>
              <Space wrap style={{ marginBottom: 12 }}>
                <Text strong>关键词：</Text>
                {paper.keywords.length > 0
                  ? paper.keywords.map((keyword) => <Tag color="geekblue" key={keyword}>{keyword}</Tag>)
                  : <Text type="secondary">未提取</Text>}
              </Space>
              <Row gutter={[16, 12]}>
                <Col xs={24} md={12}><Text strong>方法：</Text>{paper.methods.join("；") || "未提取"}</Col>
                <Col xs={24} md={12}><Text strong>实验/数据：</Text>{paper.dataAndExperiments.join("；") || "未提取"}</Col>
                <Col xs={24} md={12}><Text strong>结论：</Text>{paper.conclusions.join("；") || "未提取"}</Col>
                <Col xs={24} md={12}><Text strong>局限：</Text>{paper.limitations.join("；") || "未提取"}</Col>
              </Row>
            </List.Item>
          )}
        />
      </Card>
      <Card title="共同关键词重点分析">
        {result.keywordOverlaps.length === 0 ? (
          <Empty description="这些论文暂未发现规范化后的共同关键词" />
        ) : (
          <List
            dataSource={result.keywordOverlaps}
            renderItem={(overlap) => (
              <List.Item>
                <List.Item.Meta
                  title={<Space wrap><Tag color="purple">{overlap.keyword}</Tag>{overlap.paperIds.map((id) => <Tag key={id}>{id}</Tag>)}</Space>}
                  description={overlap.analysis}
                />
              </List.Item>
            )}
          />
        )}
      </Card>
      <Card title="跨论文异同">
        <List
          dataSource={result.comparisons}
          renderItem={(comparison) => (
            <List.Item>
              <List.Item.Meta
                title={comparison.dimension}
                description={
                  <Space direction="vertical" size={4}>
                    <Text><Text strong>共同点：</Text>{comparison.commonPoints.join("；") || "无"}</Text>
                    <Text><Text strong>差异：</Text>{comparison.differences.join("；") || "无"}</Text>
                    <Text type={comparison.conflicts.length ? "danger" : "secondary"}><Text strong>冲突：</Text>{comparison.conflicts.join("；") || "无"}</Text>
                  </Space>
                }
              />
            </List.Item>
          )}
        />
      </Card>
      <Card title={<Space><Lightbulb size={18} />对当前项目的修改建议</Space>}>
        <List
          dataSource={result.recommendations}
          renderItem={(recommendation) => (
            <List.Item>
              <List.Item.Meta
                title={<Space wrap><Text strong>{recommendation.title}</Text><Tag color="green">置信度 {Math.round(recommendation.confidence * 100)}%</Tag></Space>}
                description={
                  <Space direction="vertical" size={4}>
                    <Text>{recommendation.action}</Text>
                    <Text type="secondary">依据：{recommendation.rationale}</Text>
                    <div>{recommendation.supportingPaperIds.map((id) => <Tag key={id}>{id}</Tag>)}</div>
                  </Space>
                }
              />
            </List.Item>
          )}
        />
      </Card>
      <Card title={<Space><Network size={18} />论文知识图谱</Space>}>
        <ResearchGraph nodes={result.graphNodes} edges={result.graphEdges} />
      </Card>
    </Space>
  );
}

interface ResearchGraphSelection {
  kind: "node" | "edge";
  title: string;
  description: string;
}

const RESEARCH_NODE_TYPE_LABELS: Record<string, string> = {
  Project: "当前项目",
  Paper: "论文",
  Keyword: "共同关键词",
  Method: "方法",
  Conclusion: "结论",
  Limitation: "局限",
};

function ResearchGraph({ nodes, edges }: { nodes: ResearchGraphNode[]; edges: ResearchGraphEdge[] }) {
  const { token } = antdTheme.useToken();
  const containerRef = useRef<HTMLDivElement>(null);
  const graphRef = useRef<Core | null>(null);
  const [selection, setSelection] = useState<ResearchGraphSelection | null>(null);

  useEffect(() => {
    if (!containerRef.current || nodes.length === 0) return;
    graphRef.current?.destroy();
    setSelection(null);
    const nodeIds = new Set(nodes.map((node) => node.id));
    const positions = buildResearchGraphPositions(nodes);
    const cy = cytoscape({
      container: containerRef.current,
      elements: [
        ...nodes.map((node) => {
          const appearance = researchNodeAppearance(node.nodeType);
          return {
            data: {
              id: node.id,
              label: wrapResearchLabel(node.label),
              fullLabel: node.label,
              type: node.nodeType,
              color: appearance.color,
              width: appearance.width,
              height: appearance.height,
            },
            position: positions.get(node.id),
          };
        }),
        ...edges
          .filter((edge) => nodeIds.has(edge.source) && nodeIds.has(edge.target))
          .map((edge) => ({
            data: {
              id: edge.id,
              source: edge.source,
              target: edge.target,
              label: edge.relationType,
              reason: edge.reason,
              color: researchEdgeColor(edge.relationType),
            },
          })),
      ],
      style: [
        {
          selector: "node",
          style: {
            label: "data(label)",
            "text-wrap": "wrap",
            "text-max-width": "108px",
            "font-size": "10px",
            "font-weight": 600,
            color: "#ffffff",
            "text-valign": "center",
            "text-halign": "center",
            "background-color": "data(color)",
            shape: "round-rectangle",
            width: "data(width)",
            height: "data(height)",
            "border-color": "#ffffff",
            "border-width": "2px",
          },
        },
        {
          selector: "node:selected, node.focus-node",
          style: {
            "border-color": "#fbbf24",
            "border-width": "4px",
            "z-index": 20,
          },
        },
        {
          selector: "edge",
          style: {
            label: "",
            "font-size": "8px",
            "font-weight": 500,
            color: token.colorTextSecondary,
            "curve-style": "bezier",
            "target-arrow-shape": "triangle",
            "arrow-scale": 0.8,
            "line-color": "data(color)",
            "target-arrow-color": "data(color)",
            width: 1.6,
            opacity: 0.58,
            "text-background-color": token.colorBgContainer,
            "text-background-opacity": 0.94,
            "text-background-padding": "3px",
          },
        },
        {
          selector: "edge.focused-edge, edge.hover-edge",
          style: { label: "data(label)", opacity: 1, width: 2.5, "z-index": 12 },
        },
        { selector: ".dimmed", style: { opacity: 0.1 } },
      ],
      layout: { name: "preset", fit: true, padding: 58, animate: false },
      wheelSensitivity: 0.16,
      minZoom: 0.2,
      maxZoom: 2.5,
    });
    cy.on("tap", "node", (event) => {
      const node = event.target;
      cy.elements().removeClass("dimmed focus-node focused-edge");
      cy.elements().addClass("dimmed");
      node.closedNeighborhood().removeClass("dimmed");
      node.addClass("focus-node");
      node.connectedEdges().addClass("focused-edge");
      setSelection({
        kind: "node",
        title: String(node.data("fullLabel") ?? node.data("label")),
        description: RESEARCH_NODE_TYPE_LABELS[String(node.data("type"))] ?? String(node.data("type") || "图谱节点"),
      });
    });
    cy.on("tap", "edge", (event) => {
      const edge = event.target;
      setSelection({
        kind: "edge",
        title: String(edge.data("label")),
        description: String(edge.data("reason") || "暂无关系说明"),
      });
    });
    cy.on("mouseover", "edge", (event) => event.target.addClass("hover-edge"));
    cy.on("mouseout", "edge", (event) => event.target.removeClass("hover-edge"));
    cy.on("tap", (event) => {
      if (event.target !== cy) return;
      cy.elements().removeClass("dimmed focus-node focused-edge");
      setSelection(null);
    });
    graphRef.current = cy;
    return () => {
      cy.destroy();
      if (graphRef.current === cy) graphRef.current = null;
    };
  }, [nodes, edges, token.colorBgContainer, token.colorTextSecondary]);

  if (nodes.length === 0) return <Empty description="AI 未生成图谱节点" />;
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Space size={[6, 6]} wrap>
          <Tag color="#ef4444">当前项目</Tag>
          <Tag color="#2563eb">论文</Tag>
          <Tag color="#9333ea">共同关键词</Tag>
          <Tag color="#0f766e">方法</Tag>
          <Tag color="#15803d">结论</Tag>
          <Tag color="#d97706">局限</Tag>
        </Space>
        <Space>
          <Typography.Text type="secondary">点击节点聚焦关联，悬浮关系线查看类型</Typography.Text>
          <Button size="small" icon={<RotateCcw size={14} />} onClick={() => resetResearchGraph(graphRef.current, nodes)}>
            重新排布
          </Button>
          <Button size="small" icon={<Maximize2 size={14} />} onClick={() => graphRef.current?.fit(undefined, 58)}>
            适应
          </Button>
        </Space>
      </div>
      {selection && (
        <Alert
          type={selection.kind === "node" ? "info" : "success"}
          showIcon
          message={selection.title}
          description={selection.description}
          closable
          onClose={() => {
            graphRef.current?.elements().removeClass("dimmed focus-node focused-edge");
            setSelection(null);
          }}
        />
      )}
      <div
        ref={containerRef}
        className="h-[560px] w-full rounded-xl border"
        style={{ background: token.colorBgContainer }}
      />
    </div>
  );
}

function researchNodeAppearance(nodeType: string) {
  if (nodeType === "Project") return { color: "#ef4444", width: 132, height: 56 };
  if (nodeType === "Paper") return { color: "#2563eb", width: 142, height: 52 };
  if (nodeType === "Keyword") return { color: "#9333ea", width: 124, height: 42 };
  if (nodeType === "Method") return { color: "#0f766e", width: 136, height: 46 };
  if (nodeType === "Conclusion") return { color: "#15803d", width: 136, height: 46 };
  if (nodeType === "Limitation") return { color: "#d97706", width: 136, height: 46 };
  return { color: "#64748b", width: 132, height: 44 };
}

function researchNodeLayer(nodeType: string) {
  if (nodeType === "Paper") return 0;
  if (nodeType === "Keyword") return 1;
  if (nodeType === "Project") return 3;
  return 2;
}

function buildResearchGraphPositions(nodes: ResearchGraphNode[]) {
  const layers = new Map<number, ResearchGraphNode[]>();
  nodes.forEach((node) => {
    const layer = researchNodeLayer(node.nodeType);
    const items = layers.get(layer) ?? [];
    items.push(node);
    layers.set(layer, items);
  });
  const positions = new Map<string, { x: number; y: number }>();
  const maxLayerSize = Math.max(1, ...[...layers.values()].map((items) => items.length));
  const totalHeight = Math.max(520, maxLayerSize * 92);
  const layerX = [110, 410, 730, 1030];
  layers.forEach((items, layer) => {
    const gap = totalHeight / (items.length + 1);
    items.forEach((node, index) => {
      positions.set(node.id, { x: layerX[layer] ?? 730, y: gap * (index + 1) });
    });
  });
  return positions;
}

function resetResearchGraph(cy: Core | null, nodes: ResearchGraphNode[]) {
  if (!cy) return;
  const positions = buildResearchGraphPositions(nodes);
  cy.nodes().positions((node) => positions.get(node.id()) ?? node.position());
  cy.elements().removeClass("dimmed focus-node focused-edge");
  cy.fit(undefined, 58);
}

function researchEdgeColor(relationType: string) {
  if (relationType === "CONTRADICTS") return "#ef4444";
  if (relationType === "HAS_KEYWORD") return "#a855f7";
  if (relationType === "APPLICABLE_TO_PROJECT") return "#22c55e";
  if (relationType === "HAS_LIMITATION") return "#f59e0b";
  return "#94a3b8";
}

function wrapResearchLabel(label: string, maxLength = 20, lineLength = 10) {
  const characters = Array.from(label.replace(/\s+/g, " ").trim());
  const visible = characters.slice(0, maxLength);
  const lines: string[] = [];
  for (let index = 0; index < visible.length; index += lineLength) {
    lines.push(visible.slice(index, index + lineLength).join(""));
  }
  if (characters.length > maxLength && lines.length > 0) lines[lines.length - 1] += "…";
  return lines.join("\n");
}

function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}
