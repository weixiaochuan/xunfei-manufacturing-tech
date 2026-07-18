/**
 * PluginPanelViewPage —— 插件面板视图渲染页面
 *
 * 路由 /plugin-view/:viewId
 * 从 PluginManager 查找已注册的插件面板视图，调用其 render 函数
 * 把 DOM 内容注入到容器 div 中。
 *
 * 使用模式：插件侧边栏按钮点击 → navigate(`#/plugin-view/${viewId}`)
 */

import { useEffect, useRef, useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { Button, Result, Typography, theme as antdTheme } from "antd";
import { ArrowLeft, Puzzle } from "lucide-react";
import { pluginManager } from "@/services/pluginManager";
import type { PluginPanelViewDef } from "@/types";

const { Text } = Typography;

export default function PluginPanelViewPage() {
  const { viewId } = useParams<{ viewId: string }>();
  const navigate = useNavigate();
  const { token } = antdTheme.useToken();
  const containerRef = useRef<HTMLDivElement>(null);
  const [viewDef, setViewDef] = useState<PluginPanelViewDef | null>(null);
  const [title, setTitle] = useState("插件视图");
  const [renderKey, setRenderKey] = useState(0);

  // 查询视图定义并触发重渲染（每次 viewId 变化或注册表变化都重新查询）
  useEffect(() => {
    if (!viewId) return;
    const lookup = () => {
      const def = pluginManager.getPanelView(viewId);
      if (def) {
        setViewDef(def);
        setTitle(def.title);
        setRenderKey((k) => k + 1); // 强制 DOM 重新挂载
      } else {
        const allIds = pluginManager
          .getRegisteredPanelViews()
          .map((v) => `${v.pluginId ?? "?"}:${v.id}`);
        console.warn(
          `[PluginPanelViewPage] viewId="${viewId}" 未命中。当前已注册视图:`,
          allIds,
          " 已激活插件:",
          pluginManager.getActiveIds(),
        );
        setViewDef(null);
      }
    };
    lookup();
    // 订阅注册表变化：插件后注册视图（异步 onLoad）时自动重试
    const unsubscribe = pluginManager.subscribe("views", lookup);
    return unsubscribe;
  }, [viewId]);

  // 挂载 / 卸载插件视图 DOM（用 renderKey 保证可重入）
  useEffect(() => {
    if (!viewDef || !containerRef.current) return;
    const container = containerRef.current;
    container.innerHTML = "";
    const cleanup = viewDef.render(container);
    return () => {
      if (typeof cleanup === "function") {
        cleanup();
      } else {
        container.innerHTML = "";
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewDef, renderKey]);

  if (!viewId || !viewDef) {
    const activeIds = pluginManager.getActiveIds();
    const subTitle = !viewId
      ? "未指定视图 ID"
      : activeIds.length === 0
      ? "当前没有任何插件运行时被激活。请到插件页启用插件后重试。"
      : `已激活的插件 (${activeIds.join(", ")}) 中均未注册名为 "${viewId}" 的视图。`;
    return (
      <div
        className="flex flex-col items-center justify-center"
        style={{ height: "100%", padding: 32 }}
      >
        <Result
          icon={<Puzzle size={48} style={{ opacity: 0.3 }} />}
          title="视图未找到"
          subTitle={subTitle}
          extra={
            <Button
              type="primary"
              icon={<ArrowLeft size={14} />}
              onClick={() => navigate("/notes")}
            >
              返回笔记
            </Button>
          }
        />
      </div>
    );
  }

  const isFullscreen = viewDef.layout === "fullscreen";

  return (
    <div
      style={{
        height: "100%",
        minHeight: 0,
        display: "flex",
        flexDirection: "column",
      }}
    >
      {!isFullscreen && (
        <div
          style={{
            padding: "8px 16px",
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
            display: "flex",
            alignItems: "center",
            gap: 8,
            background: token.colorBgContainer,
          }}
        >
          <Button
            type="text"
            size="small"
            icon={<ArrowLeft size={16} />}
            onClick={() => navigate("/notes")}
          />
          <Puzzle size={16} style={{ color: token.colorPrimary }} />
          <Text strong style={{ fontSize: 15 }}>
            {title}
          </Text>
          <div style={{ flex: 1 }} />
          <Text type="secondary" style={{ fontSize: 12 }}>
            插件: {viewDef.pluginId}
          </Text>
        </div>
      )}

      <div
        ref={containerRef}
        style={{
          flex: 1,
          minHeight: 0,
          height: "100%",
          overflow: isFullscreen ? "hidden" : "auto",
          padding: isFullscreen ? 0 : 16,
        }}
      />
    </div>
  );
}
