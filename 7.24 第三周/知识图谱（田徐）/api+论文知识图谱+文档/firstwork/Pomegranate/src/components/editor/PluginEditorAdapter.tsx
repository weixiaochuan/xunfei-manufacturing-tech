
/**
 * 插件编辑器适配器
 * 集成插件系统，支持编辑器插件
 */

import React, { useEffect } from "react";
import { PluginManager } from "@/services/pluginManager";

/**
 * 插件编辑器管理器
 * 管理编辑器插件的注册和注销
 */
class PluginEditorManager {
  private pluginManager: PluginManager;
  private registeredPlugins: Set<string> = new Set();

  constructor(pluginManager: PluginManager) {
    this.pluginManager = pluginManager;
  }

  /**
   * 初始化：从插件管理器加载编辑器插件
   */
  init(): void {
    // 获取所有已激活的插件
    const activePlugins = this.pluginManager.getActiveIds();

    // 为每个插件尝试注册编辑器
    activePlugins.forEach((pluginId) => {
      this.registerPluginEditors(pluginId);
    });

    console.log(
      `[PluginEditorManager] Initialized with ${activePlugins.length} active plugins`
    );
  }

  /**
   * 注册插件提供的编辑器
   */
  private registerPluginEditors(pluginId: string): void {
    if (this.registeredPlugins.has(pluginId)) return;

    // 获取插件信息
    // 这里我们需要通过某种方式获取插件提供的编辑器
    // 暂时只处理编辑器扩展

    this.registeredPlugins.add(pluginId);
  }

  /**
   * 从插件注册表获取编辑器工具栏按钮
   */
  getPluginToolbarButtons() {
    return this.pluginManager.getRegisteredEditorToolbarButtons();
  }

  /**
   * 从插件注册表获取编辑器菜单项
   */
  getPluginContextMenuItems() {
    return this.pluginManager.getRegisteredEditorMenuItems();
  }
}

// 全局单例（延迟初始化）
let pluginEditorManager: PluginEditorManager | null = null;

/**
 * 获取插件编辑器管理器
 */
export function getPluginEditorManager(
  pluginManager: PluginManager
): PluginEditorManager {
  if (!pluginEditorManager) {
    pluginEditorManager = new PluginEditorManager(pluginManager);
    pluginEditorManager.init();
  }
  return pluginEditorManager;
}

/**
 * 插件编辑器适配器组件
 * 集成插件系统到编辑器
 */
interface PluginEditorAdapterProps {
  children: React.ReactNode;
  pluginManager: PluginManager;
}

export function PluginEditorAdapter({
  children,
  pluginManager,
}: PluginEditorAdapterProps) {
  // 初始化插件编辑器管理器
  useEffect(() => {
    getPluginEditorManager(pluginManager);
  }, [pluginManager]);

  return <>{children}</>;
}

/**
 * 使用插件编辑器工具栏按钮
 */
export function usePluginToolbarButtons() {
  // 这里实现从插件管理器获取工具栏按钮的逻辑
  // 暂时返回空数组
  return [];
}

/**
 * 使用插件编辑器菜单项
 */
export function usePluginContextMenuItems() {
  // 这里实现从插件管理器获取菜单项的逻辑
  // 暂时返回空数组
  return [];
}
