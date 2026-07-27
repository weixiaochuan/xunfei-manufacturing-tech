import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Input,
  List,
  Popconfirm,
  Row,
  Select,
  Space,
  Statistic,
  Tag,
  Typography,
  message,
  theme as antdTheme,
} from "antd";
import { ArrowLeft, BookOpen, Database, RefreshCw, Search, Trash2 } from "lucide-react";

import { trashApi } from "@/lib/api";
import {
  getResearchLibraryFolder,
  listResearchLibraryNotes,
  markdownField,
  RESEARCH_LIBRARY_FOLDER,
} from "@/lib/researchKnowledgeBase";
import type { Note } from "@/types";

const { Title, Paragraph, Text } = Typography;

interface ResearchLibraryItem {
  note: Note;
  paperType: string;
  year: string;
  doi: string;
  authors: string;
  sources: string;
  tags: string[];
}

function toLibraryItem(note: Note): ResearchLibraryItem {
  const tagText = markdownField(note.content, "建议标签");
  return {
    note,
    paperType: note.content.includes("<!-- research-analysis -->")
      ? "分析与图谱"
      : markdownField(note.content, "类型") || "未分类",
    year: markdownField(note.content, "发表时间") || "年份未知",
    doi: markdownField(note.content, "DOI"),
    authors: markdownField(note.content, "作者") || "作者信息未收录",
    sources: markdownField(note.content, "论文库来源") || "来源未收录",
    tags: [...tagText.matchAll(/#([^\s#]+)/g)].map((match) => match[1]),
  };
}

export default function ResearchLibraryPage() {
  const navigate = useNavigate();
  const { token } = antdTheme.useToken();
  const [notes, setNotes] = useState<Note[]>([]);
  const [loading, setLoading] = useState(true);
  const [folderExists, setFolderExists] = useState(false);
  const [query, setQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<string>();

  async function loadLibrary() {
    setLoading(true);
    try {
      const folder = await getResearchLibraryFolder();
      setFolderExists(Boolean(folder));
      setNotes(folder ? await listResearchLibraryNotes(folder.id) : []);
    } catch (error) {
      message.error(`读取论文知识库失败：${String(error)}`);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadLibrary();
  }, []);

  const items = useMemo(() => notes.map(toLibraryItem), [notes]);
  const typeOptions = useMemo(
    () =>
      [...new Set(items.map((item) => item.paperType))]
        .sort((left, right) => left.localeCompare(right, "zh-CN"))
        .map((value) => ({ label: value, value })),
    [items],
  );
  const visibleItems = useMemo(() => {
    const normalizedQuery = query.trim().toLocaleLowerCase();
    return items.filter((item) => {
      if (typeFilter && item.paperType !== typeFilter) return false;
      if (!normalizedQuery) return true;
      return [
        item.note.title,
        item.note.content,
        item.authors,
        item.doi,
        item.sources,
        ...item.tags,
      ].some((value) => value.toLocaleLowerCase().includes(normalizedQuery));
    });
  }, [items, query, typeFilter]);

  async function moveToTrash(note: Note) {
    try {
      await trashApi.softDelete(note.id);
      setNotes((current) => current.filter((item) => item.id !== note.id));
      message.success("已移至回收站，可在回收站恢复");
    } catch (error) {
      message.error(`移除失败：${String(error)}`);
    }
  }

  return (
    <div className="mx-auto max-w-[1180px] pb-8">
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <Space align="start" size={12}>
          <Database size={30} color={token.colorPrimary} />
          <div>
            <Title level={2} style={{ margin: 0 }}>
              论文知识库
            </Title>
            <Paragraph type="secondary" style={{ margin: "4px 0 0" }}>
              集中查看、检索和整理由你确认加入的论文；删除会先进入回收站。
            </Paragraph>
          </div>
        </Space>
        <Space>
          <Button icon={<ArrowLeft size={15} />} onClick={() => navigate("/research-assistant")}>
            返回 AI 助研
          </Button>
          <Button icon={<RefreshCw size={15} />} loading={loading} onClick={() => void loadLibrary()}>
            刷新
          </Button>
        </Space>
      </div>

      {!folderExists && !loading ? (
        <Alert
          className="mb-4"
          type="info"
          showIcon
          message="论文知识库尚未创建"
          description="在 AI 助研中完成论文评估并点击“加入知识库”后，系统会自动创建专用文件夹。"
          action={<Button type="primary" onClick={() => navigate("/research-assistant")}>去检索论文</Button>}
        />
      ) : null}

      <Card className="mb-4">
        <Row gutter={[16, 16]} align="middle">
          <Col xs={12} sm={6}>
            <Statistic title="知识条目" value={items.length} suffix="条" />
          </Col>
          <Col xs={12} sm={6}>
            <Statistic title="论文分类" value={typeOptions.length} suffix="类" />
          </Col>
          <Col xs={24} sm={12}>
            <Space.Compact block>
              <Input
                allowClear
                prefix={<Search size={15} />}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="搜索题名、作者、DOI、来源或标签"
              />
              <Select
                allowClear
                style={{ minWidth: 150 }}
                value={typeFilter}
                options={typeOptions}
                onChange={setTypeFilter}
                placeholder="全部类型"
              />
            </Space.Compact>
          </Col>
        </Row>
      </Card>

      <Card title={RESEARCH_LIBRARY_FOLDER}>
        <List
          loading={loading}
          itemLayout="vertical"
          dataSource={visibleItems}
          locale={{
            emptyText: (
              <Empty
                image={<BookOpen size={56} color={token.colorTextQuaternary} />}
                description={items.length > 0 ? "没有符合当前筛选条件的论文" : "尚无已入库论文"}
              />
            ),
          }}
          renderItem={(item) => (
            <List.Item
              key={item.note.id}
              actions={[
                <Button key="open" type="link" onClick={() => navigate(`/notes/${item.note.id}`)}>
                  查看与编辑
                </Button>,
                <Popconfirm
                  key="trash"
                  title="移至回收站？"
                  description="论文笔记不会永久删除，可在回收站恢复。"
                  okText="移至回收站"
                  cancelText="取消"
                  onConfirm={() => void moveToTrash(item.note)}
                >
                  <Button type="link" danger icon={<Trash2 size={14} />}>
                    移除
                  </Button>
                </Popconfirm>,
              ]}
            >
              <List.Item.Meta
                title={<Text strong>{item.note.title.replace(/^论文[｜|]\s*/u, "")}</Text>}
                description={
                  <Space direction="vertical" size={6} style={{ width: "100%" }}>
                    <Text type="secondary">{item.authors}</Text>
                    <Space wrap>
                      <Tag color="geekblue">{item.paperType}</Tag>
                      <Tag>{item.year}</Tag>
                      {item.doi && item.doi !== "未收录" ? <Tag color="blue">DOI {item.doi}</Tag> : null}
                      {item.tags.map((tag) => <Tag key={tag}>{tag}</Tag>)}
                    </Space>
                    <Text type="secondary">来源：{item.sources}</Text>
                    <Text type="secondary">更新于 {item.note.updated_at}</Text>
                  </Space>
                }
              />
            </List.Item>
          )}
        />
      </Card>
    </div>
  );
}
