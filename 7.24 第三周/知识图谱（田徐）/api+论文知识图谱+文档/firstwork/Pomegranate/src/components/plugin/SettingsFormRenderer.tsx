/**
 * SettingsFormRenderer —— 基于 schema 的 antd Form 自动渲染工具
 *
 * 供插件 `app.settings.createForm(container, schema)` 内部调用。
 *
 * 设计：
 *  1. ReactDOM.createRoot 挂到 container
 *  2. 启动时从 settings.getAll() 异步读初始值
 *  3. schema.fields 逐字段翻译成 antd Form.Item
 *  4. 任何值变更 → debounce 300ms 后 settings.set(key, value)
 *  5. 返回 cleanup 函数（root.unmount + container 清理）
 */

import { useEffect, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Form, Input, InputNumber, Switch, Select, Spin } from "antd";
import type { PluginSettingsFormSchema, SettingsFormField } from "@/types";

/** 插件 settings 读写接口（与 PluginSettingsAPI 对齐，只取 get/set） */
interface SettingsStore {
  get<T = unknown>(key: string): Promise<T | undefined>;
  set(key: string, value: unknown): Promise<void>;
}

function SettingsForm({
  schema,
  store,
}: {
  schema: PluginSettingsFormSchema;
  store: SettingsStore;
}) {
  const [form] = Form.useForm();
  const [loading, setLoading] = useState(true);

  // 启动时拉取已有设置作为初始值
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const initial: Record<string, unknown> = {};
        for (const f of schema.fields) {
          const val = await store.get(f.key);
          if (val !== undefined) initial[f.key] = val;
          else if ("default" in f && f.default !== undefined)
            initial[f.key] = f.default;
        }
        if (!cancelled) {
          form.setFieldsValue(initial);
          setLoading(false);
        }
      } catch {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 任何值变化 → 自动保存到 settings
  function handleValuesChange(_changed: unknown, allValues: Record<string, unknown>) {
    for (const [key, value] of Object.entries(allValues)) {
      void store.set(key, value).catch(() => {});
    }
  }

  if (loading) {
    return (
      <div style={{ padding: 16, textAlign: "center" }}>
        <Spin size="small" />
      </div>
    );
  }

  return (
    <Form
      form={form}
      layout="vertical"
      size="small"
      onValuesChange={handleValuesChange}
      style={{ maxWidth: 480 }}
    >
      {schema.fields.map(renderField)}
    </Form>
  );
}

function renderField(f: SettingsFormField) {
  const common = { key: f.key, name: f.key, label: f.label };

  if (f.kind === "text") {
    return (
      <Form.Item {...common}>
        <Input placeholder={f.placeholder} />
      </Form.Item>
    );
  }
  if (f.kind === "number") {
    return (
      <Form.Item {...common}>
        <InputNumber min={f.min} max={f.max} style={{ width: "100%" }} />
      </Form.Item>
    );
  }
  if (f.kind === "boolean") {
    return (
      <Form.Item {...common} valuePropName="checked">
        <Switch />
      </Form.Item>
    );
  }
  // select
  return (
    <Form.Item {...common}>
      <Select options={f.options} />
    </Form.Item>
  );
}

/**
 * 暴露给 pluginApi.ts 的入口：把 schema + store 渲染到给定 container，
 * 返回 unmount 函数。
 */
export function createSettingsForm(
  container: HTMLElement,
  schema: PluginSettingsFormSchema,
  store: SettingsStore,
): () => void {
  container.innerHTML = "";
  const root: Root = createRoot(container);
  root.render(<SettingsForm schema={schema} store={store} />);
  return () => {
    root.unmount();
    container.innerHTML = "";
  };
}
