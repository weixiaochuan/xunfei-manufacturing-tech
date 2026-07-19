import { useState } from "react";
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Form,
  Input,
  List,
  Row,
  Space,
  Statistic,
  Tag,
  Typography,
  message,
  theme as antdTheme,
} from "antd";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowUpRight,
  BookOpen,
  CalendarDays,
  Microscope,
  Search,
  Sparkles,
  Users,
} from "lucide-react";

import { researchApi } from "@/lib/api";
import type { ResearchPaper, ResearchPaperSearchResult } from "@/types";

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

export default function ResearchAssistantPage() {
  const { token } = antdTheme.useToken();
  const [form] = Form.useForm<ResearchFormValues>();
  const [loading, setLoading] = useState(false);
  const [searchResult, setSearchResult] = useState<ResearchPaperSearchResult | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  async function handleSearch(values: ResearchFormValues) {
    setLoading(true);
    setErrorMessage(null);
    try {
      const result = await researchApi.searchPapers({
        query: buildQuery(values),
        limit: 12,
      });
      setSearchResult(result);
      if (result.papers.length > 0) {
        message.success(`已找到 ${result.papers.length} 篇近五年论文`);
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

  return (
    <div className="mx-auto max-w-[1180px] pb-8">
      <Space align="start" size={12} className="mb-4">
        <Microscope size={30} color={token.colorPrimary} />
        <div>
          <Title level={2} style={{ margin: 0 }}>
            AI 助研
          </Title>
          <Paragraph type="secondary" style={{ margin: "4px 0 0" }}>
            面向机械工程及相关方向检索近五年的前沿论文，快速查看作者、出处、引用情况并跳转原文。
          </Paragraph>
        </div>
      </Space>

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
          message="当前版本使用 Crossref 公开论文元数据检索，会自动限定当前年份及之前四个自然年；不需要 API Key，也不会读取 AI 资源中心凭据。"
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
          <Card>
            <Row gutter={[16, 16]} align="middle">
              <Col xs={12} sm={6}>
                <Statistic title="检索年份" value={`${searchResult.fromYear}-${searchResult.toYear}`} />
              </Col>
              <Col xs={12} sm={6}>
                <Statistic title="匹配记录" value={formatCount(searchResult.totalResults)} />
              </Col>
              <Col xs={12} sm={6}>
                <Statistic title="精选展示" value={searchResult.papers.length} suffix="篇" />
              </Col>
              <Col xs={12} sm={6}>
                <Statistic title="数据来源" value={searchResult.source} />
              </Col>
            </Row>
          </Card>

          {searchResult.papers.length === 0 ? (
            <Card>
              <Empty description="没有找到匹配论文，请缩短主题或换一组关键词再试" />
            </Card>
          ) : (
            <Card title="近五年前沿论文" extra={<Text type="secondary">综合相关度、年份和引用量排序</Text>}>
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
                            <Tag>Crossref 引用 {formatCount(paper.citedByCount)}</Tag>
                            <Tag color="purple">{paper.rankReason}</Tag>
                          </Space>
                        </Space>
                      }
                    />
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
