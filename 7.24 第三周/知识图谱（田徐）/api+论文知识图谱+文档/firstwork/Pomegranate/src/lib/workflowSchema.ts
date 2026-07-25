import type {
  WorkflowInputField,
  WorkflowInputFieldType,
  WorkflowInputOption,
} from "@/types";

export const DEFAULT_WORKFLOW_INPUT_KEY = "AGENT_USER_INPUT";

export const WORKFLOW_FIELD_TYPE_OPTIONS: Array<{ value: WorkflowInputFieldType; label: string }> = [
  { value: "string", label: "单行文本" },
  { value: "multiline", label: "多行文本" },
  { value: "integer", label: "整数" },
  { value: "number", label: "数字" },
  { value: "boolean", label: "布尔值" },
  { value: "select", label: "下拉选择" },
  { value: "json", label: "JSON" },
  { value: "file", label: "单文件" },
  { value: "files", label: "多文件" },
];

const FIELD_TYPES = new Set(WORKFLOW_FIELD_TYPE_OPTIONS.map((item) => item.value));
const MAX_FIELDS = 50;

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function asString(value: unknown, fallback = "") {
  return typeof value === "string" ? value : fallback;
}

function normalizeOptions(value: unknown): WorkflowInputOption[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((item) => {
      if (typeof item === "string") return { label: item, value: item };
      const record = asRecord(item);
      if (!record) return null;
      const optionValue = asString(record.value, asString(record.key));
      if (!optionValue.trim()) return null;
      return {
        label: asString(record.label, optionValue),
        value: optionValue,
      };
    })
    .filter((item): item is WorkflowInputOption => Boolean(item));
}

function normalizeField(item: unknown, index: number): WorkflowInputField | null {
  const record = asRecord(item);
  if (!record) return null;
  const key = asString(record.key, asString(record.name)).trim();
  if (!isValidWorkflowFieldKey(key)) return null;
  const rawType = asString(record.type, asString(record.fieldType, "string")) as WorkflowInputFieldType;
  const type = FIELD_TYPES.has(rawType) ? rawType : "string";
  const defaultValue = record.defaultValue ?? record.default_value ?? record.default;
  const sensitive = Boolean(record.sensitive);
  const rawFileConfig = asRecord(record.fileConfig ?? record.file_config) ?? {};
  return {
    key,
    label: asString(record.label, key),
    type,
    required: record.required !== false,
    defaultValue,
    placeholder: asString(record.placeholder),
    description: asString(record.description),
    options: normalizeOptions(record.options),
    order: typeof record.order === "number" ? record.order : index,
    sensitive,
    fileConfig: {
      allowedExtensions: Array.isArray(rawFileConfig.allowedExtensions)
        ? rawFileConfig.allowedExtensions.map(String).filter(Boolean)
        : Array.isArray(rawFileConfig.allowed_extensions)
          ? rawFileConfig.allowed_extensions.map(String).filter(Boolean)
          : typeof rawFileConfig.allowedExtensions === "string"
            ? rawFileConfig.allowedExtensions.split(",").map((ext) => ext.trim()).filter(Boolean)
            : typeof rawFileConfig.allowed_extensions === "string"
              ? rawFileConfig.allowed_extensions.split(",").map((ext) => ext.trim()).filter(Boolean)
              : [],
      maxSizeMb: typeof rawFileConfig.maxSizeMb === "number"
        ? rawFileConfig.maxSizeMb
        : typeof rawFileConfig.max_size_mb === "number"
          ? rawFileConfig.max_size_mb
          : typeof rawFileConfig.maxSizeMb === "string" && rawFileConfig.maxSizeMb.trim()
            ? Number(rawFileConfig.maxSizeMb)
            : typeof rawFileConfig.max_size_mb === "string" && rawFileConfig.max_size_mb.trim()
              ? Number(rawFileConfig.max_size_mb)
              : undefined,
      multiple: rawFileConfig.multiple === true || type === "files",
      valueMode: asString(rawFileConfig.valueMode, asString(rawFileConfig.value_mode)) as any,
    },
  };
}

export function isValidWorkflowFieldKey(key: string) {
  return /^[A-Za-z0-9_-]+$/.test(key.trim());
}

export function normalizeWorkflowInputFields(raw: unknown, fallbackKey = DEFAULT_WORKFLOW_INPUT_KEY): WorkflowInputField[] {
  const fields = Array.isArray(raw)
    ? raw
    : asRecord(raw)?.fields && Array.isArray(asRecord(raw)?.fields)
      ? (asRecord(raw)?.fields as unknown[])
      : [];
  const seen = new Set<string>();
  const normalized = fields
    .slice(0, MAX_FIELDS)
    .map(normalizeField)
    .filter((field): field is WorkflowInputField => Boolean(field))
    .filter((field) => {
      if (seen.has(field.key)) return false;
      seen.add(field.key);
      return true;
    })
    .sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
  if (normalized.length > 0) return normalized;
  return [{
    key: isValidWorkflowFieldKey(fallbackKey) ? fallbackKey : DEFAULT_WORKFLOW_INPUT_KEY,
    label: "用户输入",
    type: "multiline",
    required: true,
    placeholder: "请输入工作流开始节点需要的文本",
    order: 0,
  }];
}

export function workflowFieldsFromMapping(jsonText: string, fallbackKey = DEFAULT_WORKFLOW_INPUT_KEY): WorkflowInputField[] {
  try {
    const mapping = JSON.parse(jsonText || "{}");
    const record = asRecord(mapping) ?? {};
    const inputSchema = asRecord(record.inputSchema ?? record.input_schema);
    if (inputSchema?.fields) {
      return normalizeWorkflowInputFields(inputSchema.fields, fallbackKey);
    }
    const legacyFields = Array.isArray(record.inputFields)
      ? record.inputFields
      : Array.isArray(record.input_fields)
        ? record.input_fields
        : null;
    if (legacyFields) {
      return normalizeWorkflowInputFields(
        legacyFields.map((item) => {
          const legacy = asRecord(item) ?? {};
          const name = asString(legacy.name, asString(legacy.key));
          return {
            key: name,
            label: name,
            type: legacy.source === "user_input" ? "multiline" : "string",
            required: legacy.required !== false,
            defaultValue: legacy.value ?? legacy.defaultValue ?? legacy.default_value,
          };
        }),
        fallbackKey,
      );
    }
    return normalizeWorkflowInputFields([], asString(record.inputParameter, asString(record.input_parameter, fallbackKey)));
  } catch {
    return normalizeWorkflowInputFields([], fallbackKey);
  }
}

export function workflowFieldsFromConfigurationSchema(schema: unknown, fallbackKey = DEFAULT_WORKFLOW_INPUT_KEY): WorkflowInputField[] {
  const record = asRecord(schema) ?? {};
  const inputSchema = asRecord(record.inputSchema ?? record.input_schema);
  if (inputSchema?.fields) return normalizeWorkflowInputFields(inputSchema.fields, fallbackKey);
  const defaultInputSchema = asRecord(inputSchema?.default);
  if (defaultInputSchema?.fields) return normalizeWorkflowInputFields(defaultInputSchema.fields, fallbackKey);
  const inputParameter = asRecord(record.inputParameter ?? record.input_parameter);
  return normalizeWorkflowInputFields([], asString(inputParameter?.default, fallbackKey));
}

export function buildWorkflowRequestMapping(fields: WorkflowInputField[], fallbackKey = DEFAULT_WORKFLOW_INPUT_KEY) {
  const normalized = normalizeWorkflowInputFields(fields, fallbackKey);
  const inputParameter =
    normalized.find((field) => field.type === "multiline" || field.type === "string")?.key ??
    normalized[0]?.key ??
    DEFAULT_WORKFLOW_INPUT_KEY;
  const legacyInputFields = normalized
    .filter((field) => field.type !== "file" && field.type !== "files")
    .map((field) => ({
      name: field.key,
      source: field.key === inputParameter ? "user_input" : "constant",
      value: field.key === inputParameter ? undefined : field.defaultValue,
      required: Boolean(field.required),
    }));
  return JSON.stringify({
    inputParameter,
    inputFields: legacyInputFields,
    inputSchema: { fields: normalized },
  });
}

export function importWorkflowFieldsFromText(text: string): WorkflowInputField[] {
  const trimmed = text.trim();
  if (!trimmed) return [];
  try {
    const parsed = JSON.parse(trimmed);
    const record = asRecord(parsed);
    if (Array.isArray(parsed)) return normalizeWorkflowInputFields(parsed);
    if (record?.inputSchema || record?.input_schema) {
      return workflowFieldsFromMapping(JSON.stringify(record));
    }
    if (record?.fields || record?.inputFields || record?.input_fields) {
      return normalizeWorkflowInputFields(record.fields ?? record.inputFields ?? record.input_fields);
    }
    if (record?.properties && asRecord(record.properties)) {
      const required = Array.isArray(record.required) ? record.required.map(String) : [];
      return normalizeWorkflowInputFields(
        Object.entries(asRecord(record.properties) ?? {}).map(([key, value], index) => {
          const prop = asRecord(value) ?? {};
          const enumValues = Array.isArray(prop.enum) ? prop.enum.map(String) : [];
          return {
            key,
            label: asString(prop.title, key),
            type: jsonSchemaTypeToFieldType(prop.type, enumValues.length > 0),
            required: required.includes(key),
            placeholder: asString(prop.description),
            description: asString(prop.description),
            options: enumValues.map((item) => ({ label: item, value: item })),
            order: index,
          };
        }),
      );
    }
  } catch {
    const yamlFields = parseSimpleYamlFields(trimmed);
    if (yamlFields.length > 0) return normalizeWorkflowInputFields(yamlFields);
  }
  return [];
}

function jsonSchemaTypeToFieldType(type: unknown, hasEnum: boolean): WorkflowInputFieldType {
  if (hasEnum) return "select";
  if (type === "integer") return "integer";
  if (type === "number") return "number";
  if (type === "boolean") return "boolean";
  if (type === "object" || type === "array") return "json";
  return "string";
}

function parseSimpleYamlFields(text: string): WorkflowInputField[] {
  const fields: Array<Partial<WorkflowInputField>> = [];
  let current: Partial<WorkflowInputField> | null = null;
  for (const line of text.split(/\r?\n/)) {
    const itemMatch = line.match(/^\s*-\s*(?:key|name)\s*:\s*["']?([A-Za-z0-9_-]+)["']?\s*$/);
    if (itemMatch) {
      current = { key: itemMatch[1], label: itemMatch[1], type: "string", required: true };
      fields.push(current);
      continue;
    }
    const inlineMatch = line.match(/^\s*([A-Za-z0-9_-]+)\s*:\s*(string|text|multiline|integer|number|boolean|select|json|file|files)?\s*(?:#.*)?$/);
    if (inlineMatch && !["parameters", "properties", "inputs", "fields"].includes(inlineMatch[1])) {
      fields.push({
        key: inlineMatch[1],
        label: inlineMatch[1],
        type: yamlTypeToFieldType(inlineMatch[2]),
        required: true,
      });
      continue;
    }
    if (!current) continue;
    const propMatch = line.match(/^\s*(label|type|required|description|placeholder)\s*:\s*(.+?)\s*$/);
    if (!propMatch) continue;
    const [, prop, rawValue] = propMatch;
    const value = rawValue.replace(/^["']|["']$/g, "");
    if (prop === "type") current.type = yamlTypeToFieldType(value);
    else if (prop === "required") current.required = !/^false$/i.test(value);
    else (current as Record<string, unknown>)[prop] = value;
  }
  return normalizeWorkflowInputFields(fields);
}

function yamlTypeToFieldType(type: string | undefined): WorkflowInputFieldType {
  if (!type) return "string";
  const lower = type.toLowerCase();
  if (lower === "text") return "string";
  return FIELD_TYPES.has(lower as WorkflowInputFieldType) ? (lower as WorkflowInputFieldType) : "string";
}

export function workflowInitialValues(fields: WorkflowInputField[]) {
  return fields.reduce<Record<string, unknown>>((acc, field) => {
    if (field.defaultValue !== undefined && field.defaultValue !== null) {
      acc[field.key] = field.defaultValue;
    } else if (field.type === "boolean") {
      acc[field.key] = false;
    } else if (field.type === "file" || field.type === "files") {
      acc[field.key] = [];
    }
    return acc;
  }, {});
}

export function buildWorkflowSubmission(fields: WorkflowInputField[], rawValues: Record<string, unknown>) {
  const parameters: Record<string, unknown> = {};
  const filePaths: Record<string, string[]> = {};
  for (const field of fields) {
    const raw = rawValues[field.key];
    if (field.type === "file" || field.type === "files") {
      const paths = Array.isArray(raw)
        ? raw.map(String).filter(Boolean)
        : typeof raw === "string" && raw.trim()
          ? [raw.trim()]
          : [];
      if (paths.length === 0) {
        if (field.required) throw new Error(`${field.label || field.key} 为必填文件字段`);
        continue;
      }
      filePaths[field.key] = field.type === "file" ? paths.slice(0, 1) : paths;
      continue;
    }
    if (raw === undefined || raw === null || raw === "") {
      if (field.defaultValue !== undefined && field.defaultValue !== null && field.defaultValue !== "") {
        parameters[field.key] = field.defaultValue;
        continue;
      }
      if (field.required) throw new Error(`${field.label || field.key} 为必填字段`);
      continue;
    }
    parameters[field.key] = convertWorkflowValue(field, raw);
  }
  return { parameters, filePaths };
}

function convertWorkflowValue(field: WorkflowInputField, raw: unknown) {
  if (field.type === "integer") {
    const number = Number(raw);
    if (!Number.isInteger(number)) throw new Error(`${field.label || field.key} 必须是整数`);
    return number;
  }
  if (field.type === "number") {
    const number = Number(raw);
    if (!Number.isFinite(number)) throw new Error(`${field.label || field.key} 必须是数字`);
    return number;
  }
  if (field.type === "boolean") {
    return Boolean(raw);
  }
  if (field.type === "json") {
    if (typeof raw !== "string") return raw;
    try {
      return JSON.parse(raw);
    } catch {
      throw new Error(`${field.label || field.key} 不是合法 JSON`);
    }
  }
  return String(raw);
}

export function workflowPreview(
  fields: WorkflowInputField[],
  parameters: Record<string, unknown>,
  filePaths: Record<string, string[]> = {},
) {
  const fieldMap = new Map(fields.map((field) => [field.key, field]));
  const preview: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(parameters)) {
    preview[key] = fieldMap.get(key)?.sensitive ? "***" : value;
  }
  for (const [key, paths] of Object.entries(filePaths)) {
    preview[key] = paths.map((path) => `待上传:${path.split(/[\\/]/).pop() || "file"}`);
  }
  return preview;
}
