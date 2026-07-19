/**
 * RibbonBar —— 插件功能区按钮组
 *
 * 渲染位置：主窗 Header 右侧（Settings 按钮之前），与系统功能按钮以细分隔线隔开。
 *
 * 数据源：pluginManager.subscribe("ribbon") 拉取已激活插件注册的 ribbon 项；
 * 插件停用后自动清空（不依赖 onUnload 显式调用 removeItem）。
 *
 * 行为：点击执行 onClick；同步抛错或 Promise 拒绝都用 message.error 提示，
 * 不影响其他插件 / 主应用。
 */

import { useEffect, useState } from "react";
import { Tooltip, Button, theme as antdTheme, message } from "antd";
import { pluginManager } from "@/services/pluginManager";
import { resolvePluginIconComponent } from "@/components/plugin/pluginIcons";
import type { PluginRibbonItemDef } from "@/types";

type RibbonEntry = PluginRibbonItemDef & { pluginId: string };

export function RibbonBar() {
  const { token } = antdTheme.useToken();
  const [items, setItems] = useState<RibbonEntry[]>([]);

  useEffect(() => {
    const refresh = () => setItems(pluginManager.getRegisteredRibbonItems());
    refresh();
    return pluginManager.subscribe("ribbon", refresh);
  }, []);

  async function handleClick(item: RibbonEntry) {
    try {
      await item.onClick();
    } catch (e) {
      console.error(`[RibbonBar] 插件 ${item.pluginId} ribbon ${item.id} 抛错:`, e);
      pluginManager._logError(item.pluginId, "ribbon:onClick", String(e));
      message.error(`插件「${item.pluginId}」按钮执行失败：${e}`);
    }
  }

  if (items.length === 0) return null;

  return (
    <>
      {items.map((item) => {
        const Icon = resolvePluginIconComponent(item.icon);
        const pluginName = pluginManager.getPluginName(item.pluginId) ?? item.pluginId;
        return (
          <Tooltip
            key={`${item.pluginId}:${item.id}`}
            title={
              <span>
                <span style={{ opacity: 0.7, fontSize: 11 }}>{pluginName}</span>
                <br />
                {item.tooltip}
              </span>
            }
            mouseEnterDelay={0.15}
          >
            <Button
              type="text"
              icon={<Icon size={16} />}
              onClick={() => void handleClick(item)}
              aria-label={`${pluginName}: ${item.tooltip}`}
            />
          </Tooltip>
        );
      })}
      {/* 与右侧系统按钮（搜索/主题/同步/设置...）之间的视觉分隔 */}
      <div
        aria-hidden
        style={{
          width: 1,
          height: 18,
          margin: "0 4px",
          background: token.colorBorderSecondary,
        }}
      />
    </>
  );
}
