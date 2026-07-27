import { useState, useEffect, useCallback, useMemo, Fragment } from "react";
import { useNavigate } from "react-router-dom";
import {
  Input,
  Typography,
  Button,
  Tag,
  Modal,
  App as AntdApp,
  theme as antdTheme,
} from "antd";
import {
  NotebookText,
  CalendarDays,
  ArrowRight,
  CheckSquare,
  AlertTriangle,
  PencilLine,
  Clock,
  TrendingUp,
  Sparkles,
  Check,
  Star,
  Type,
  Copy,
  Edit3,
  Trash2,
  ExternalLink,
  ChevronRight,
  ChevronDown,
} from "lucide-react";
import { Tooltip as AntTooltip } from "antd";
import {
  noteApi,
  dailyApi,
  systemApi,
  taskApi,
  trashApi,
  tagApi,
} from "@/lib/api";
import { useContextMenu } from "@/hooks/useContextMenu";
import {
  ContextMenuOverlay,
  type ContextMenuEntry,
} from "@/components/ui/ContextMenuOverlay";
import { relativeTime } from "@/lib/utils";
import { EmptyState } from "@/components/ui/EmptyState";
import { NewNoteButton } from "@/components/NewNoteButton";
import { MicButton } from "@/components/MicButton";
import { HomeSearchInput } from "@/components/HomeSearchInput";
import { useAppStore } from "@/store";
import {
  useHomeLayoutStore,
  type HomeWidgetId,
  COL_SPAN_MAP,
  HEIGHT_MAP,
} from "@/store/homeLayout";
import {
  DndContext,
  closestCenter,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  rectSortingStrategy,
  arrayMove,
} from "@dnd-kit/sortable";
import { RotateCcw } from "lucide-react";
import { SortableWidget } from "@/components/home/SortableWidget";
import { HomeWidget } from "@/components/home/HomeWidget";
import { useFeatureEnabled } from "@/hooks/useFeatureEnabled";
import type {
  Note,
  DashboardStats,
  DailyWritingStat,
  Task,
} from "@/types";
import { SubtaskList } from "@/components/tasks/SubtaskList";
import { TaskDetailModal } from "@/components/tasks/TaskDetailModal";
import { CreateTaskModal } from "@/components/tasks/CreateTaskModal";

const { Text } = Typography;

/**
 * 首页 v2(工作台模式)
 *
 * 结构(从上到下):
 *   ① 搜索 + 新建笔记拆分按钮(全局入口)
 *   ② 快速记一笔(textarea, ⌘↩ 追加到今日 daily)
 *   ③ 写作活力(笔记/连续天数/距上次/本周字数 + 14 天迷你图)
 *   ④ 最近编辑(2/3) + 待办速览(1/3)
 */
/**
 * 桌面端原版主页（保留所有原有 hook + 1300+ 行实现）。
 * 移动端走文件末尾的 HomePage wrapper → MobileHome（按设计稿小屏布局）。
 */
function DesktopHomePage() {
  const navigate = useNavigate();
  const { token } = antdTheme.useToken();
  const { message } = AntdApp.useApp();
  const refreshTaskStats = useAppStore((s) => s.refreshTaskStats);

  // 功能模块开关：用户在设置里关闭某模块时，首页相关的卡片/按钮也要联动隐藏
  const tasksEnabled = useFeatureEnabled("tasks");
  const dailyEnabled = useFeatureEnabled("daily");

  // ─── 数据状态 ─────────────────────────────────────
  const [recentNotes, setRecentNotes] = useState<Note[]>([]);
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [trend, setTrend] = useState<DailyWritingStat[]>([]);
  /** 今日待办速览(今天 + 逾期,前端筛 / 切片) */
  const [todayTasks, setTodayTasks] = useState<Task[]>([]);
  /** 即将到期速览(明天到 7 天内,前端筛 / 切片) */
  const [upcomingTasks, setUpcomingTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(true);

  // ─── 首页布局（可拖拽排序 + 折叠 + 宽高） ────────────
  const {
    widgets,
    collapsed,
    hydrate: hydrateLayout,
    setOrder,
    setColSpan,
    setHeight,
    toggleCollapse,
    resetLayout,
  } = useHomeLayoutStore();

  const dndSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
  );

  /** 首页只显示四个核心组件，并继续尊重待办和日记功能开关。 */
  const visibleWidgets = useMemo(() => {
    return widgets.filter((w) => {
      switch (w.id) {
        case "quick-note":
          return dailyEnabled;
        case "todo":
          return tasksEnabled;
        case "recent":
        case "writing-stat":
          return true;
        default:
          return false;
      }
    });
  }, [widgets, dailyEnabled, tasksEnabled]);

  const visibleWidgetIds = useMemo(
    () => visibleWidgets.map((w) => w.id),
    [visibleWidgets],
  );
  const [searchKeyword, setSearchKeyword] = useState("");
  const [quickNote, setQuickNote] = useState("");
  const [quickNoteSaving, setQuickNoteSaving] = useState(false);
  /** 待办详情弹窗 */
  const [taskDetail, setTaskDetail] = useState<Task | null>(null);
  /** 编辑弹窗（详情 Modal 点「编辑」时打开）*/
  const [editingTask, setEditingTask] = useState<Task | null>(null);
  /** 行内展开子任务的任务 id 集合（仅 widget 内存态，不持久化） */
  const [expandedTodoIds, setExpandedTodoIds] = useState<Set<number>>(
    () => new Set(),
  );

  // ─── 加载 ─────────────────────────────────────────
  const loadDashboard = useCallback(async () => {
    setLoading(true);
    try {
      const [notesResult, dashStats, trendData, allTodos] =
        await Promise.all([
          noteApi.list({ page: 1, page_size: 8 }),
          systemApi.getDashboardStats(),
          systemApi.getWritingTrend(14),
          taskApi.list({ status: 0 }).catch(() => [] as Task[]),
        ]);
      setRecentNotes(notesResult.items);
      setStats(dashStats);
      setTrend(trendData);
      // 三段筛：逾期+今日 给 todayTasks；明天到 7 天后 给 upcomingTasks
      const todayEnd = new Date();
      todayEnd.setHours(23, 59, 59, 999);
      const next7End = new Date(todayEnd);
      next7End.setDate(next7End.getDate() + 7);
      const allWithDue = allTodos.filter((t) => t.status === 0 && t.due_date);
      const sortByDue = (a: Task, b: Task) =>
        new Date(a.due_date!).getTime() - new Date(b.due_date!).getTime();
      // 逾期 + 今日（≤ 今天结束）
      const todayOrOverdue = allWithDue
        .filter((t) => new Date(t.due_date!).getTime() <= todayEnd.getTime())
        .sort(sortByDue);
      // 即将到期（今天结束 < due ≤ 7 天后结束）
      const upcoming = allWithDue
        .filter((t) => {
          const d = new Date(t.due_date!).getTime();
          return d > todayEnd.getTime() && d <= next7End.getTime();
        })
        .sort(sortByDue);
      setTodayTasks(todayOrOverdue);
      setUpcomingTasks(upcoming);
    } catch (e) {
      console.error("加载首页数据失败:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadDashboard();
    void hydrateLayout();
  }, [loadDashboard, hydrateLayout]);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const allIds = widgets.map((w) => w.id);
      const oldIdx = allIds.indexOf(active.id as HomeWidgetId);
      const newIdx = allIds.indexOf(over.id as HomeWidgetId);
      if (oldIdx === -1 || newIdx === -1) return;
      setOrder(arrayMove(allIds, oldIdx, newIdx));
    },
    [widgets, setOrder],
  );

  const handleResetLayout = useCallback(() => {
    Modal.confirm({
      title: "重置首页布局",
      content: "将所有 Widget 的位置、宽度、高度和折叠状态恢复为默认。",
      okText: "重置",
      cancelText: "取消",
      onOk: () => {
        resetLayout();
        message.success("已重置为默认布局");
      },
    });
  }, [resetLayout, message]);

  // ─── 搜索 ─────────────────────────────────────────
  const handleSearch = useCallback(async () => {
    const keyword = searchKeyword.trim();
    if (!keyword) return;

    if (keyword.startsWith("#")) {
      const tagName = keyword.slice(1).trim();
      if (!tagName) {
        message.info("请输入标签名，例如 #机械制造");
        return;
      }
      try {
        const tags = await tagApi.list();
        const tag = tags.find((item) => item.name.toLowerCase() === tagName.toLowerCase());
        if (tag) {
          navigate(`/tags?tagId=${tag.id}`);
        } else {
          message.warning(`没有找到标签：${tagName}`);
        }
      } catch (error) {
        message.error(`读取标签失败：${error}`);
      }
      return;
    }

    navigate(`/search?q=${encodeURIComponent(keyword)}`);
  }, [searchKeyword, navigate, message]);

  // ─── 快速记一笔:追加到今日 daily ───────────────────
  const handleQuickSaveNote = useCallback(async () => {
    const text = quickNote.trim();
    if (!text) return;
    setQuickNoteSaving(true);
    try {
      const today = new Date().toISOString().slice(0, 10);
      const daily = await dailyApi.getOrCreate(today);
      const now = new Date();
      const hhmm = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
      // 用时间戳前缀分隔多条快速记录,内容追加到 content 末尾(content 是 HTML)
      const appendBlock = `<p><strong>[${hhmm}]</strong> ${text.replace(/\n/g, "<br>")}</p>`;
      const newContent = (daily.content || "") + appendBlock;
      await noteApi.update(daily.id, {
        title: daily.title,
        content: newContent,
        folder_id: daily.folder_id,
      });
      message.success("已写入今日笔记");
      setQuickNote("");
      // 也刷新最近笔记列表(daily 会出现在最近)
      loadDashboard();
    } catch (e) {
      message.error(`保存失败: ${e}`);
    } finally {
      setQuickNoteSaving(false);
    }
  }, [quickNote, message, loadDashboard]);

  // ─── 完成 / 切换待办状态 ──────────────────────────
  const handleToggleTask = useCallback(
    async (id: number) => {
      try {
        await taskApi.toggleStatus(id);
        refreshTaskStats();
        loadDashboard();
      } catch (e) {
        message.error(`操作失败: ${e}`);
      }
    },
    [message, refreshTaskStats, loadDashboard],
  );

  /** 同时 patch todayTasks 和 upcomingTasks（一个任务可能在任一段里） */
  const patchTask = useCallback((id: number, patch: Partial<Task>) => {
    setTodayTasks((prev) =>
      prev.map((t) => (t.id === id ? { ...t, ...patch } : t)),
    );
    setUpcomingTasks((prev) =>
      prev.map((t) => (t.id === id ? { ...t, ...patch } : t)),
    );
  }, []);

  // ─── 右键菜单（最近笔记 + 今日待办两个 widget） ─────
  const noteCtx = useContextMenu<Note>();
  const taskCtx = useContextMenu<Task>();

  const noteMenuItems: ContextMenuEntry[] = useMemo(() => {
    const p = noteCtx.state.payload;
    if (!p) return [];
    return [
      {
        key: "open",
        label: "打开笔记",
        icon: <ExternalLink size={13} />,
        onClick: () => {
          noteCtx.close();
          navigate(`/notes/${p.id}`);
        },
      },
      {
        key: "copy-wiki",
        label: "复制为 wiki 链接",
        icon: <Copy size={13} />,
        onClick: () => {
          noteCtx.close();
          const link = `[[${p.title || "无标题"}]]`;
          navigator.clipboard
            .writeText(link)
            .then(() => message.success(`已复制：${link}`))
            .catch((e) => message.error(`复制失败：${e}`));
        },
      },
      { type: "divider" },
      {
        key: "trash",
        label: "移到回收站",
        icon: <Trash2 size={13} />,
        danger: true,
        onClick: () => {
          noteCtx.close();
          Modal.confirm({
            title: `把「${p.title || "(无标题)"}」移到回收站？`,
            content: "可以在回收站恢复。",
            okText: "移入回收站",
            okButtonProps: { danger: true },
            async onOk() {
              try {
                await trashApi.softDelete(p.id);
                message.success("已移到回收站");
                loadDashboard();
              } catch (e) {
                message.error(`删除失败：${e}`);
              }
            },
          });
        },
      },
    ];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [noteCtx.state.payload]);

  const taskMenuItems: ContextMenuEntry[] = useMemo(() => {
    const p = taskCtx.state.payload;
    if (!p) return [];
    const done = p.status === 1;
    return [
      {
        key: "toggle",
        label: done ? "标记为未完成" : "标记已完成",
        icon: <Check size={13} />,
        onClick: () => {
          taskCtx.close();
          void handleToggleTask(p.id);
        },
      },
      {
        key: "edit",
        label: "在待办页打开",
        icon: <Edit3 size={13} />,
        onClick: () => {
          taskCtx.close();
          navigate(`/tasks?taskId=${p.id}`);
        },
      },
      { type: "divider" },
      {
        key: "delete",
        label: "删除任务",
        icon: <Trash2 size={13} />,
        danger: true,
        onClick: () => {
          taskCtx.close();
          Modal.confirm({
            title: `删除任务「${p.title || "(无标题)"}」？`,
            content: "此操作不可恢复。",
            okText: "删除",
            okButtonProps: { danger: true },
            async onOk() {
              try {
                await taskApi.delete(p.id);
                message.success("已删除");
                refreshTaskStats();
                loadDashboard();
              } catch (e) {
                message.error(`删除失败：${e}`);
              }
            },
          });
        },
      },
    ];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [taskCtx.state.payload]);

  // ─── 派生指标(纯前端从 trend 计算) ────────────────
  const vitalityMetrics = useMemo(() => {
    // trend 是按日期升序,只含有笔记的日期
    const todayStr = new Date().toISOString().slice(0, 10);
    const trendByDate = new Map(trend.map((d) => [d.date, d]));

    // 连续写作天数:从今天往前数,直到某天没写作
    let streak = 0;
    const cursor = new Date();
    for (let i = 0; i < 365; i++) {
      const ymd = cursor.toISOString().slice(0, 10);
      const d = trendByDate.get(ymd);
      if (d && d.word_count > 0) {
        streak++;
        cursor.setDate(cursor.getDate() - 1);
      } else {
        // 如果今天还没写,允许从昨天起算(避免一进首页就 = 0)
        if (i === 0) {
          cursor.setDate(cursor.getDate() - 1);
          continue;
        }
        break;
      }
    }

    // 距上次写作:用最近笔记 updated_at
    const lastWritingAt = recentNotes[0]?.updated_at;
    const lastSinceLabel = lastWritingAt ? relativeTime(lastWritingAt) : "—";

    // 本周字数(最近 7 天) + 上周字数(8-14 天)对比
    let thisWeek = 0;
    let lastWeek = 0;
    const now = new Date();
    for (let i = 0; i < 14; i++) {
      const d = new Date(now);
      d.setDate(now.getDate() - i);
      const ymd = d.toISOString().slice(0, 10);
      const stat = trendByDate.get(ymd);
      if (!stat) continue;
      if (i < 7) thisWeek += stat.word_count;
      else lastWeek += stat.word_count;
    }
    const weekDelta = lastWeek > 0
      ? Math.round(((thisWeek - lastWeek) / lastWeek) * 100)
      : null;

    return {
      totalNotes: stats?.total_notes ?? 0,
      totalWords: stats?.total_words ?? 0,
      streak,
      lastSinceLabel,
      thisWeekWords: thisWeek,
      weekDelta,
      todayStr,
      trendByDate,
    };
  }, [trend, stats, recentNotes]);

  // 14 天柱状图的 normalize
  const trendBars = useMemo(() => {
    const now = new Date();
    const items: { date: string; words: number }[] = [];
    for (let i = 13; i >= 0; i--) {
      const d = new Date(now);
      d.setDate(now.getDate() - i);
      const ymd = d.toISOString().slice(0, 10);
      const stat = vitalityMetrics.trendByDate.get(ymd);
      items.push({ date: ymd, words: stat?.word_count ?? 0 });
    }
    const maxW = Math.max(...items.map((i) => i.words), 1);
    return items.map((it) => ({ ...it, ratio: it.words / maxW }));
  }, [vitalityMetrics]);

  // 待办速览三段：逾期（昨天及更早）/ 今日 / 即将到期。每段最多 5 条
  const todoGroups = useMemo(() => {
    const todayStart = new Date();
    todayStart.setHours(0, 0, 0, 0);
    const todayStartMs = todayStart.getTime();
    const overdue: Task[] = [];
    const today: Task[] = [];
    for (const t of todayTasks) {
      // todayTasks 已经是 due ≤ 今天结束 的，按 todayStart 切：早于今天 0:00 = 逾期
      if (new Date(t.due_date!).getTime() < todayStartMs) overdue.push(t);
      else today.push(t);
    }
    return {
      overdue: overdue.slice(0, 5),
      overdueTotal: overdue.length,
      today: today.slice(0, 5),
      todayTotal: today.length,
      upcoming: upcomingTasks.slice(0, 5),
      upcomingTotal: upcomingTasks.length,
    };
  }, [todayTasks, upcomingTasks]);
  const totalTodoCount = todoGroups.overdueTotal + todoGroups.todayTotal + todoGroups.upcomingTotal;
  const displayedRecent = useMemo(() => recentNotes.slice(0, 5), [recentNotes]);

  // ─── 渲染 ─────────────────────────────────────────
  return (
    <div
      // 外层 wrapper 撑满 Content 区域 —— max-w-5xl 居中后两侧空白属于 Content
      // 不属于内层 div，事件不会冒泡。把 onContextMenu 放在撑满的 wrapper 上
      // 才能拦到这部分空白的右键
      style={{ width: "100%", minHeight: "100%" }}
      onContextMenu={(e) => {
        const t = e.target as HTMLElement;
        if (t.closest("input, textarea, [contenteditable='true']")) return;
        e.preventDefault();
      }}
    >
    <div
      className="max-w-5xl mx-auto"
      style={{ display: "flex", flexDirection: "column", gap: 12 }}
    >

      {/* ① 顶部搜索 + 新建笔记
          搜索：输入即下拉建议（笔记 + 待办，点击跳详情），回车去 /search 全量结果 */}
      <div className="flex gap-3">
        <HomeSearchInput
          value={searchKeyword}
          onChange={setSearchKeyword}
          onPressEnter={handleSearch}
        />
        <NewNoteButton size="large" style={{ borderRadius: 8 }} />
      </div>

      {/* 可拖拽排序 + 可调宽高的 Widget 区（CSS Grid 12 列） */}
      <div className="flex items-center justify-end gap-2 -mb-1">
        <AntTooltip title="重置为默认布局" mouseEnterDelay={0.4}>
          <Button
            type="text"
            size="small"
            icon={<RotateCcw size={13} />}
            onClick={handleResetLayout}
          >
            <span style={{ fontSize: 12 }}>重置布局</span>
          </Button>
        </AntTooltip>
      </div>
      <DndContext
        sensors={dndSensors}
        collisionDetection={closestCenter}
        onDragEnd={handleDragEnd}
      >
        <SortableContext
          items={visibleWidgetIds}
          strategy={rectSortingStrategy}
        >
          <div className="grid grid-cols-12 gap-3 auto-rows-min">
            {visibleWidgets.map((w) => {
              const isCollapsed = collapsed[w.id] ?? false;
              // Tailwind 4 不能扫描动态拼接的类名，必须用静态映射
              const COL_CLASS: Record<number, string> = {
                4: "col-span-4",
                6: "col-span-6",
                8: "col-span-8",
                12: "col-span-12",
              };
              const colSpanClass = COL_CLASS[COL_SPAN_MAP[w.colSpan]] ?? "col-span-12";
              const minH = HEIGHT_MAP[w.heightLevel];

              const renderContent = (): React.ReactNode => {
                switch (w.id) {
                  case "quick-note":
                    return (
                      <>
                        <div className="flex items-center justify-end gap-2 mb-2">
                          {quickNote.trim() && (
                            <Text type="secondary" style={{ fontSize: 11 }}>
                              {quickNote.trim().length} 字
                            </Text>
                          )}
                          <Text type="secondary" style={{ fontSize: 11 }}>
                            Ctrl/⌘ + ↩
                          </Text>
                          <MicButton
                            onTranscribed={(text) =>
                              setQuickNote((prev) => (prev ? `${prev}\n${text}` : text))
                            }
                          />
                          <Button
                            size="small"
                            type="primary"
                            ghost
                            icon={<Check size={13} />}
                            loading={quickNoteSaving}
                            disabled={!quickNote.trim()}
                            onClick={handleQuickSaveNote}
                          >
                            保存
                          </Button>
                        </div>
                        <Input.TextArea
                          rows={2}
                          placeholder="想到什么先记下来…"
                          value={quickNote}
                          onChange={(e) => setQuickNote(e.target.value)}
                          onKeyDown={(e) => {
                            if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                              e.preventDefault();
                              handleQuickSaveNote();
                            }
                          }}
                          style={{ borderRadius: 6, fontSize: 13 }}
                          autoSize={{ minRows: 2, maxRows: 8 }}
                        />
                      </>
                    );

                  case "todo": {
                    if (totalTodoCount === 0) {
                      return (
                        <div className="text-center py-4">
                          <Text type="secondary" style={{ fontSize: 12 }}>
                            {loading ? "加载中…" : "暂无待办，享受当下 ✨"}
                          </Text>
                        </div>
                      );
                    }
                    const renderTaskRow = (
                      task: Task,
                      sectionKey: "overdue" | "today" | "upcoming",
                    ) => {
                      const due = new Date(task.due_date!);
                      const dueMs = due.getTime();
                      const dotColor =
                        task.priority === 0
                          ? token.colorError
                          : task.priority === 1
                            ? token.colorPrimary
                            : token.colorTextTertiary;
                      const desc = task.description?.trim();
                      const ctxActive = taskCtx.state.payload?.id === task.id;
                      const hasSubtasks = task.subtask_total > 0;
                      const expanded = expandedTodoIds.has(task.id);

                      let dateLabel: React.ReactNode;
                      if (sectionKey === "overdue") {
                        const todayStart = new Date();
                        todayStart.setHours(0, 0, 0, 0);
                        const days = Math.max(
                          1,
                          Math.round((todayStart.getTime() - dueMs) / 86400000),
                        );
                        dateLabel = (
                          <span
                            className="text-[11px] px-1.5 py-0.5 rounded shrink-0"
                            style={{
                              background: `${token.colorError}1a`,
                              color: token.colorError,
                            }}
                          >
                            {days === 1 ? "昨天" : `${days} 天前`}
                          </span>
                        );
                      } else if (sectionKey === "today") {
                        const hh = String(due.getHours()).padStart(2, "0");
                        const mm = String(due.getMinutes()).padStart(2, "0");
                        const noTime = hh === "00" && mm === "00";
                        const overdueNow = dueMs < Date.now();
                        dateLabel = (
                          <Text
                            type={overdueNow ? "danger" : "secondary"}
                            style={{ fontSize: 11, flexShrink: 0 }}
                          >
                            {noTime ? "今天" : `${hh}:${mm}`}
                          </Text>
                        );
                      } else {
                        const todayStart = new Date();
                        todayStart.setHours(0, 0, 0, 0);
                        const days = Math.max(
                          1,
                          Math.ceil((dueMs - todayStart.getTime()) / 86400000),
                        );
                        dateLabel = (
                          <Text
                            type="secondary"
                            style={{ fontSize: 11, flexShrink: 0 }}
                          >
                            +{days} 天
                          </Text>
                        );
                      }

                      return (
                        <Fragment key={task.id}>
                          <li
                            className="flex items-start gap-2.5"
                            style={{
                              padding: "4px 6px",
                              borderRadius: 4,
                              background: ctxActive ? token.colorPrimaryBg : "transparent",
                              transition: "background .12s",
                            }}
                            onContextMenu={(e) => {
                              e.preventDefault();
                              taskCtx.open(e.nativeEvent, task);
                            }}
                          >
                            {hasSubtasks ? (
                              <button
                                onClick={(e) => {
                                  e.stopPropagation();
                                  setExpandedTodoIds((prev) => {
                                    const next = new Set(prev);
                                    if (next.has(task.id)) next.delete(task.id);
                                    else next.add(task.id);
                                    return next;
                                  });
                                }}
                                className="flex items-center justify-center hover:bg-black/5 rounded transition cursor-pointer"
                                style={{
                                  width: 16,
                                  height: 16,
                                  marginTop: 3,
                                  flexShrink: 0,
                                  color: token.colorTextTertiary,
                                }}
                                title={expanded ? "折叠子任务" : "展开子任务"}
                              >
                                {expanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                              </button>
                            ) : (
                              <span style={{ width: 16, flexShrink: 0 }} />
                            )}
                            <input
                              type="checkbox"
                              onChange={() => handleToggleTask(task.id)}
                              onClick={(e) => e.stopPropagation()}
                              style={{ cursor: "pointer", flexShrink: 0, marginTop: 4 }}
                            />
                            <span
                              style={{
                                width: 6,
                                height: 6,
                                borderRadius: "50%",
                                background: dotColor,
                                flexShrink: 0,
                                marginTop: 7,
                              }}
                            />
                            <div
                              className="flex-1 min-w-0 cursor-pointer"
                              onClick={() => setTaskDetail(task)}
                            >
                              <div className="flex items-center gap-1.5">
                                <Text
                                  ellipsis
                                  style={{ fontSize: 13, flex: 1, minWidth: 0 }}
                                >
                                  {task.title}
                                </Text>
                                {task.important && (
                                  <Star
                                    size={11}
                                    style={{ color: token.colorWarning, flexShrink: 0 }}
                                    fill={token.colorWarning}
                                  />
                                )}
                                {dateLabel}
                              </div>
                              <Text
                                type="secondary"
                                ellipsis
                                style={{ fontSize: 11, display: "block", minHeight: 16 }}
                              >
                                {desc || "\u00A0"}
                              </Text>
                            </div>
                          </li>
                          {expanded && hasSubtasks && (
                            <li
                              className="list-none"
                              style={{
                                padding: "6px 10px 8px 38px",
                                background: token.colorFillQuaternary,
                                borderRadius: 4,
                              }}
                              onClick={(e) => e.stopPropagation()}
                            >
                              <SubtaskList
                                parentTaskId={task.id}
                                compact
                                onChanged={(done, total) =>
                                  patchTask(task.id, {
                                    subtask_done: done,
                                    subtask_total: total,
                                  })
                                }
                              />
                            </li>
                          )}
                        </Fragment>
                      );
                    };

                    const sections: Array<{
                      key: "overdue" | "today" | "upcoming";
                      items: Task[];
                      total: number;
                      label: string;
                      icon: React.ReactNode;
                      color: string;
                    }> = [
                      {
                        key: "overdue",
                        items: todoGroups.overdue,
                        total: todoGroups.overdueTotal,
                        label: "逾期",
                        icon: <AlertTriangle size={11} />,
                        color: token.colorError,
                      },
                      {
                        key: "today",
                        items: todoGroups.today,
                        total: todoGroups.todayTotal,
                        label: "今日",
                        icon: <CalendarDays size={11} />,
                        color: token.colorPrimary,
                      },
                      {
                        key: "upcoming",
                        items: todoGroups.upcoming,
                        total: todoGroups.upcomingTotal,
                        label: "即将到期(7 天内)",
                        icon: <Clock size={11} />,
                        color: token.colorTextSecondary,
                      },
                    ];

                    return (
                      <div className="flex flex-col gap-3" style={{ overflowY: "auto" }}>
                        {sections
                          .filter((s) => s.items.length > 0)
                          .map((s) => (
                            <div key={s.key} className="flex flex-col gap-1">
                              <div
                                className="flex items-center gap-1.5"
                                style={{
                                  fontSize: 11,
                                  color: s.color,
                                  fontWeight: 600,
                                }}
                              >
                                {s.icon}
                                <span>{s.label}</span>
                                <Text
                                  type="secondary"
                                  style={{ fontSize: 11, fontWeight: 400 }}
                                >
                                  · {s.total}
                                </Text>
                              </div>
                              <ul className="flex flex-col gap-1.5 m-0 p-0 list-none">
                                {s.items.map((t) => renderTaskRow(t, s.key))}
                              </ul>
                              {s.total > s.items.length && (
                                <Button
                                  type="link"
                                  size="small"
                                  onClick={() => navigate("/tasks")}
                                  style={{
                                    padding: 0,
                                    fontSize: 11,
                                    alignSelf: "flex-start",
                                    marginLeft: 24,
                                  }}
                                >
                                  + 还有 {s.total - s.items.length} 条…
                                </Button>
                              )}
                            </div>
                          ))}
                      </div>
                    );
                  }

                  case "recent": {
                    if (displayedRecent.length === 0) {
                      return (
                        <EmptyState description={loading ? "加载中…" : "还没有文档"} />
                      );
                    }
                    return (
                      <ul className="flex flex-col gap-2 m-0 p-0 list-none" style={{ overflowY: "auto" }}>
                        {displayedRecent.map((note) => {
                          const ctxActive = noteCtx.state.payload?.id === note.id;
                          return (
                            <li
                              key={note.id}
                              className="cursor-pointer"
                              style={{
                                padding: "4px 6px",
                                borderRadius: 4,
                                background: ctxActive ? token.colorPrimaryBg : "transparent",
                                transition: "background .12s",
                              }}
                              onClick={() => navigate(`/notes/${note.id}`)}
                              onContextMenu={(e) => {
                                e.preventDefault();
                                noteCtx.open(e.nativeEvent, note);
                              }}
                            >
                              <div className="flex items-center gap-1.5">
                                {note.is_daily && (
                                  <Tag
                                    color="blue"
                                    style={{ fontSize: 10, lineHeight: "14px", padding: "0 4px", margin: 0 }}
                                  >
                                    日记
                                  </Tag>
                                )}
                                <Text ellipsis style={{ fontSize: 13, flex: 1, minWidth: 0 }}>
                                  {note.title}
                                </Text>
                              </div>
                              <Text
                                type="secondary"
                                style={{ fontSize: 11, display: "block", minHeight: 16 }}
                              >
                                {relativeTime(note.updated_at)} · {note.word_count} 字
                              </Text>
                            </li>
                          );
                        })}
                      </ul>
                    );
                  }

                  case "writing-stat":
                    return (
                      <div className="flex flex-col gap-4 h-full">
                        {/* 上排：指标 4 项，文档总数+总字数 | 距上次+本周字数 */}
                        <div className="flex items-center gap-6 flex-wrap">
                          <MetricItem
                            icon={<NotebookText size={16} style={{ color: token.colorPrimary }} />}
                            iconBg={`${token.colorPrimary}15`}
                            value={vitalityMetrics.totalNotes}
                            label="文档总数"
                            compact
                          />
                          <MetricItem
                            icon={<Type size={16} style={{ color: "#7c3aed" }} />}
                            iconBg="#ede9fe"
                            value={vitalityMetrics.totalWords.toLocaleString()}
                            label="总字数"
                            isText
                            compact
                          />
                          <MetricItem
                            icon={<Clock size={16} style={{ color: "#2563eb" }} />}
                            iconBg="#dbeafe"
                            value={vitalityMetrics.lastSinceLabel}
                            label="距上次写作"
                            isText
                            compact
                          />
                          <MetricItem
                            icon={<TrendingUp size={16} style={{ color: token.colorSuccess }} />}
                            iconBg={`${token.colorSuccess}1a`}
                            value={vitalityMetrics.thisWeekWords.toLocaleString()}
                            label="本周字数"
                            compact
                            extra={
                              vitalityMetrics.weekDelta != null && (
                                <span
                                  className="text-[11px]"
                                  style={{
                                    color:
                                      vitalityMetrics.weekDelta >= 0
                                        ? token.colorSuccess
                                        : token.colorError,
                                  }}
                                >
                                  {vitalityMetrics.weekDelta >= 0 ? "▲" : "▼"} {Math.abs(vitalityMetrics.weekDelta)}%
                                </span>
                              )
                            }
                          />
                        </div>
                        {/* 底部：统计图 */}
                        <div
                          className="pt-3"
                          style={{ borderTop: `1px solid ${token.colorBorderSecondary}` }}
                        >
                          <div className="flex items-end gap-0.5" style={{ height: 48 }}>
                            {trendBars.map((bar) => (
                              <AntTooltip key={bar.date} title={`${bar.date} · ${bar.words} 字`}>
                                <div
                                  className="flex-1 rounded-sm transition-colors"
                                  style={{
                                    height: `${Math.max(bar.ratio * 100, 4)}%`,
                                    background: bar.words > 0
                                      ? `linear-gradient(to top, ${token.colorPrimary}, ${token.colorPrimary}cc)`
                                      : token.colorBorderSecondary,
                                    opacity: bar.words > 0 ? 0.6 + bar.ratio * 0.4 : 0.4,
                                    cursor: "pointer",
                                  }}
                                />
                              </AntTooltip>
                            ))}
                          </div>
                          <div
                            className="flex justify-between mt-1"
                            style={{ fontSize: 10, color: token.colorTextQuaternary }}
                          >
                            <span>{trendBars[0]?.date.slice(5) ?? ""}</span>
                            <span>{trendBars[7]?.date.slice(5) ?? "7 天前"}</span>
                            <span>今天</span>
                          </div>
                        </div>
                      </div>
                    );

                  default:
                    return null;
                }
              };

              const widgetMeta: Partial<Record<
                HomeWidgetId,
                { title: string; icon: React.ReactNode; extra?: React.ReactNode }
              >> = {
                "quick-note": {
                  title: "快速记一笔",
                  icon: <PencilLine size={14} style={{ color: token.colorPrimary }} />,
                  extra: (
                    <Text type="secondary" style={{ fontSize: 11 }}>
                      追加到「今日文档」
                    </Text>
                  ),
                },
                todo: {
                  title: "待办速览",
                  icon: <CheckSquare size={14} style={{ color: token.colorSuccess }} />,
                  extra: (
                    <Button
                      type="link"
                      size="small"
                      onClick={() => navigate("/tasks")}
                      style={{ padding: 0, fontSize: 12 }}
                    >
                      全部 <ArrowRight size={11} />
                    </Button>
                  ),
                },
                recent: {
                  title: "最近编辑",
                  icon: <NotebookText size={14} style={{ color: token.colorPrimary }} />,
                  extra: (
                    <Button
                      type="link"
                      size="small"
                      onClick={() => navigate("/notes")}
                      style={{ padding: 0, fontSize: 12 }}
                    >
                      更多 <ArrowRight size={11} />
                    </Button>
                  ),
                },
                "writing-stat": {
                  title: "写作活力",
                  icon: <Sparkles size={14} style={{ color: "#f97316" }} />,
                  extra: (
                    <Text type="secondary" style={{ fontSize: 11 }}>
                      近 14 天
                    </Text>
                  ),
                },
              };

              const meta = widgetMeta[w.id];
              if (!meta) return null;

              return (
                <div key={w.id} className={colSpanClass}>
                  <SortableWidget id={w.id}>
                    {({ dragHandleProps }) => (
                      <div
                        style={{
                          height: isCollapsed ? undefined : minH,
                          display: "flex",
                          flexDirection: "column",
                        }}
                      >
                        <HomeWidget
                          id={w.id}
                          title={meta.title}
                          icon={meta.icon}
                          collapsed={isCollapsed}
                          onToggle={() => toggleCollapse(w.id)}
                          dragHandleProps={dragHandleProps}
                          extra={meta.extra}
                          colSpan={w.colSpan}
                          heightLevel={w.heightLevel}
                          onColSpanChange={(s) => setColSpan(w.id, s)}
                          onHeightChange={(h) => setHeight(w.id, h)}
                        >
                          <div style={{ height: "100%", overflowY: "auto" }}>
                            {renderContent()}
                          </div>
                        </HomeWidget>
                      </div>
                    )}
                  </SortableWidget>
                </div>
              );
            })}
          </div>
        </SortableContext>
      </DndContext>


      {/* 待办详情弹窗 — 点击今日待办列表项触发；只读视图 + 标记完成 + 子任务 + 编辑入口 */}
      <TaskDetailModal
        task={taskDetail}
        onClose={() => setTaskDetail(null)}
        onToggleStatus={(id) => void handleToggleTask(id)}
        onSubtaskChanged={(id, done, total) => {
          // 局部 patch（同时覆盖 today / upcoming），避免重拉造成闪烁
          patchTask(id, { subtask_done: done, subtask_total: total });
        }}
        onEdit={(t) => setEditingTask(t)}
      />
      {/* 详情 Modal 点「编辑」→ CreateTaskModal 编辑态；保存后重拉首页数据 */}
      <CreateTaskModal
        open={!!editingTask}
        editing={editingTask}
        onClose={() => setEditingTask(null)}
        onSaved={() => {
          setEditingTask(null);
          void loadDashboard();
        }}
        onSubtaskChanged={(done, total) => {
          if (!editingTask) return;
          patchTask(editingTask.id, {
            subtask_done: done,
            subtask_total: total,
          });
        }}
      />

      {/* 最近笔记右键菜单 */}
      <ContextMenuOverlay
        open={!!noteCtx.state.payload}
        x={noteCtx.state.x}
        y={noteCtx.state.y}
        items={noteMenuItems}
        onClose={noteCtx.close}
      />

      {/* 今日待办右键菜单 */}
      <ContextMenuOverlay
        open={!!taskCtx.state.payload}
        x={taskCtx.state.x}
        y={taskCtx.state.y}
        items={taskMenuItems}
        onClose={taskCtx.close}
      />
    </div>
    </div>
  );
}

/** 写作活力卡内的单指标小项 */
function MetricItem({
  icon,
  iconBg,
  value,
  valueSuffix,
  label,
  extra,
  isText,
  compact,
}: {
  icon: React.ReactNode;
  iconBg: string;
  value: number | string;
  valueSuffix?: string;
  label: string;
  extra?: React.ReactNode;
  isText?: boolean;
  compact?: boolean;
}) {
  if (compact) {
    return (
      <div className="flex flex-col gap-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <div
            className="flex items-center justify-center rounded-md shrink-0"
            style={{ width: 22, height: 22, background: iconBg }}
          >
            {icon}
          </div>
          <span
            className="font-semibold leading-tight"
            style={{ fontSize: isText ? 13 : 16 }}
          >
            {value}
            {valueSuffix && (
              <span className="text-[11px] opacity-50 font-normal ml-0.5">{valueSuffix}</span>
            )}
            {extra && <span className="ml-1">{extra}</span>}
          </span>
        </div>
        <div className="text-[11px] opacity-60 pl-[30px]">{label}</div>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2.5">
      <div
        className="flex items-center justify-center rounded-lg shrink-0"
        style={{ width: 36, height: 36, background: iconBg }}
      >
        {icon}
      </div>
      <div className="min-w-0">
        <div className="text-base font-semibold leading-tight" style={{ fontSize: isText ? 14 : 18 }}>
          {value}
          {valueSuffix && (
            <span className="text-xs text-slate-400 font-normal ml-0.5">{valueSuffix}</span>
          )}
          {extra && <span className="ml-1.5">{extra}</span>}
        </div>
        <div className="text-[11px] text-slate-500">{label}</div>
      </div>
    </div>
  );
}

// ─── 移动端 Wrapper（T-M008）─────────────────────────
import { useIsMobile } from "@/hooks/useIsMobile";
import { MobileHome } from "./MobileHome";

/**
 * Wrapper：根据视口/平台决定渲染桌面版 or 移动版。
 * 注意：必须用 wrapper 模式而不是函数体内 early-return，
 * 否则 React Hooks 顺序会因平台不同而变化（违反 Hooks 规则）。
 */
export default function HomePage() {
  const isMobile = useIsMobile();
  return isMobile ? <MobileHome /> : <DesktopHomePage />;
}
