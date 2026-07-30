import { useMemo, useState } from "react";
import { Button, Empty, Input, InputNumber, Space, Typography } from "antd";
import { Columns3, Eye, EyeOff, Rows3, Trash2 } from "lucide-react";

const { Text } = Typography;
const EXCEL_SOURCE_TYPES = new Set(["xlsx", "xls", "xlsm", "xlsb", "ods"]);
const META_PREFIX = "<!-- firstwork-excel-table-meta:";
const META_SUFFIX = "-->";
const DEFAULT_COLUMN_WIDTH = 160;
const DEFAULT_ROW_HEIGHT = 42;

type TableMeta = {
  columnWidths?: number[];
  rowHeights?: number[];
};

type ParsedTable = {
  id: string;
  sourceKind: "markdown" | "html";
  title: string;
  blockStart: number;
  blockEnd: number;
  charStart?: number;
  charEnd?: number;
  header: string[];
  rows: string[][];
  meta: TableMeta;
};

type SelectedCell = {
  tableId: string;
  row: number;
  col: number;
};

export function isExcelSourceType(fileType?: string | null): boolean {
  return EXCEL_SOURCE_TYPES.has((fileType ?? "").toLowerCase());
}

function isSeparatorRow(line: string): boolean {
  const trimmed = line.trim();
  if (!trimmed.includes("|")) return false;
  const body = trimmed.replace(/^\|/, "").replace(/\|$/, "");
  const cells = body.split("|").map((cell) => cell.trim());
  return cells.length > 0 && cells.every((cell) => /^:?-{3,}:?$/.test(cell));
}

function isMarkdownTableRow(line: string): boolean {
  const trimmed = line.trim();
  return trimmed.includes("|") && !trimmed.startsWith("```");
}

function stripImportedHtml(value: string): string {
  let text = value
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/p>\s*<p[^>]*>/gi, "\n")
    .trim();

  if (typeof DOMParser !== "undefined" && /[<&]/.test(text)) {
    const doc = new DOMParser().parseFromString(text, "text/html");
    text = (doc.body.textContent ?? text).trim();
  } else {
    text = text
      .replace(/<[^>]+>/g, "")
      .replace(/&nbsp;/gi, " ")
      .replace(/&amp;/gi, "&")
      .replace(/&lt;/gi, "<")
      .replace(/&gt;/gi, ">")
      .replace(/&quot;/gi, '"')
      .trim();
  }

  return text.replace(/\u00a0/g, " ");
}

function splitRow(line: string): string[] {
  let body = line.trim();
  if (body.startsWith("|")) body = body.slice(1);
  if (body.endsWith("|")) body = body.slice(0, -1);

  const cells: string[] = [];
  let current = "";
  let escaping = false;
  for (const char of body) {
    if (escaping) {
      current += char;
      escaping = false;
      continue;
    }
    if (char === "\\") {
      escaping = true;
      continue;
    }
    if (char === "|") {
      cells.push(stripImportedHtml(current));
      current = "";
      continue;
    }
    current += char;
  }
  cells.push(stripImportedHtml(current));
  return cells;
}

function padCells(cells: string[], count: number): string[] {
  const next = cells.slice(0, count);
  while (next.length < count) next.push("");
  return next;
}

function parseMeta(line: string | undefined): TableMeta | null {
  const trimmed = line?.trim() ?? "";
  if (!trimmed.startsWith(META_PREFIX) || !trimmed.endsWith(META_SUFFIX)) return null;
  const json = trimmed.slice(META_PREFIX.length, -META_SUFFIX.length).trim();
  try {
    const parsed = JSON.parse(json);
    return typeof parsed === "object" && parsed ? parsed : {};
  } catch {
    return {};
  }
}

function findNearestHeading(lines: string[], beforeIndex: number, fallback: string): string {
  for (let index = beforeIndex; index >= 0; index -= 1) {
    const match = lines[index]?.match(/^\s{0,3}#{1,6}\s+(.+?)\s*$/);
    if (match?.[1]) return stripImportedHtml(match[1]);
  }
  return fallback;
}

function normalizeNumbers(values: unknown, count: number, fallback: number): number[] {
  const source = Array.isArray(values) ? values : [];
  return Array.from({ length: count }, (_, index) => {
    const value = Number(source[index]);
    return Number.isFinite(value) && value > 0 ? value : fallback;
  });
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function parseWidth(value: string | null): number | null {
  if (!value) return null;
  const match = value.match(/\d+(?:\.\d+)?/);
  if (!match) return null;
  const parsed = Number(match[0]);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
}

function columnName(index: number): string {
  let value = index + 1;
  let name = "";
  while (value > 0) {
    const remainder = (value - 1) % 26;
    name = String.fromCharCode(65 + remainder) + name;
    value = Math.floor((value - 1) / 26);
  }
  return name;
}

function lineStartOffsets(markdown: string): number[] {
  const offsets = [0];
  for (let index = 0; index < markdown.length; index += 1) {
    if (markdown[index] === "\n") offsets.push(index + 1);
  }
  return offsets;
}

function lineIndexForOffset(offsets: number[], offset: number): number {
  let result = 0;
  for (let index = 0; index < offsets.length; index += 1) {
    if (offsets[index] > offset) break;
    result = index;
  }
  return result;
}

function parseMarkdownTables(markdown: string): ParsedTable[] {
  const lines = markdown.split(/\r?\n/);
  const offsets = lineStartOffsets(markdown);
  const tables: ParsedTable[] = [];

  for (let index = 0; index < lines.length - 1; index += 1) {
    if (!isMarkdownTableRow(lines[index]) || !isSeparatorRow(lines[index + 1])) continue;

    const meta = parseMeta(lines[index - 1]);
    const blockStart = meta ? index - 1 : index;
    let blockEnd = index + 2;
    while (blockEnd < lines.length && isMarkdownTableRow(lines[blockEnd]) && !isSeparatorRow(lines[blockEnd])) {
      blockEnd += 1;
    }

    const header = splitRow(lines[index]);
    const rawRows = lines.slice(index + 2, blockEnd).map(splitRow);
    const columnCount = Math.max(1, header.length, ...rawRows.map((row) => row.length));
    tables.push({
      id: `markdown:${blockStart}:${blockEnd}:${tables.length}`,
      sourceKind: "markdown",
      title: findNearestHeading(lines, blockStart - 1, `表格 ${tables.length + 1}`),
      blockStart,
      blockEnd,
      charStart: offsets[blockStart] ?? 0,
      charEnd: offsets[blockEnd] ?? markdown.length,
      header: padCells(header, columnCount),
      rows: rawRows.map((row) => padCells(row, columnCount)),
      meta: meta ?? {},
    });

    index = blockEnd - 1;
  }

  return tables;
}

function extractHtmlCellText(cell: Element): string {
  const html = cell.innerHTML
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/p>\s*<p[^>]*>/gi, "\n");
  return stripImportedHtml(html);
}

function parseHtmlTableBlock(markdown: string, raw: string, charStart: number, charEnd: number, tableIndex: number): ParsedTable | null {
  const normalizedRaw = /<table\b/i.test(raw) ? raw : `<table><tbody>${raw}</tbody></table>`;
  const doc = new DOMParser().parseFromString(normalizedRaw, "text/html");
  const tableEl = doc.querySelector("table");
  if (!tableEl) return null;

  let rowEls = Array.from(tableEl.querySelectorAll("tr"));
  if (rowEls.length === 0) {
    const cells = Array.from(tableEl.querySelectorAll("td,th"));
    if (cells.length === 0) return null;
    const fakeRow = doc.createElement("tr");
    for (const cell of cells) fakeRow.appendChild(cell.cloneNode(true));
    rowEls = [fakeRow];
  }

  const rawRows = rowEls.map((row) => Array.from(row.querySelectorAll("th,td")));
  const columnCount = Math.max(1, ...rawRows.map((row) => row.reduce((sum, cell) => sum + Math.max(1, Number((cell as HTMLTableCellElement).colSpan) || 1), 0)));
  const grid = rawRows.map((row) => {
    const cells: string[] = [];
    for (const cell of row) {
      const span = Math.max(1, Number((cell as HTMLTableCellElement).colSpan) || 1);
      cells.push(extractHtmlCellText(cell));
      for (let index = 1; index < span; index += 1) cells.push("");
    }
    return padCells(cells, columnCount);
  });

  const firstRow = rawRows[0] ?? [];
  const columnWidths = normalizeNumbers(
    firstRow.map((cell) => parseWidth(cell.getAttribute("colwidth")) ?? parseWidth(cell.getAttribute("width")) ?? parseWidth(cell.getAttribute("style")) ?? DEFAULT_COLUMN_WIDTH),
    columnCount,
    DEFAULT_COLUMN_WIDTH,
  );
  const rowHeights = normalizeNumbers([], grid.length, DEFAULT_ROW_HEIGHT);
  const offsets = lineStartOffsets(markdown);
  const blockStart = lineIndexForOffset(offsets, charStart);
  const blockEnd = lineIndexForOffset(offsets, charEnd) + 1;
  const header = grid[0] ?? Array.from({ length: columnCount }, (_, index) => `列 ${index + 1}`);

  return {
    id: `html:${charStart}:${charEnd}:${tableIndex}`,
    sourceKind: "html",
    title: findNearestHeading(markdown.split(/\r?\n/), blockStart - 1, `HTML 表格 ${tableIndex + 1}`),
    blockStart,
    blockEnd,
    charStart,
    charEnd,
    header,
    rows: grid.slice(1),
    meta: { columnWidths, rowHeights },
  };
}

function parseHtmlTables(markdown: string): ParsedTable[] {
  if (typeof DOMParser === "undefined") return [];
  const tables: ParsedTable[] = [];
  const tablePattern = /<table\b[\s\S]*?<\/table>/gi;
  let match: RegExpExecArray | null;
  while ((match = tablePattern.exec(markdown))) {
    const table = parseHtmlTableBlock(markdown, match[0], match.index, match.index + match[0].length, tables.length);
    if (table) tables.push(table);
  }
  if (tables.length > 0) return tables;

  const fragmentMatch = markdown.match(/<tr\b[\s\S]*?<\/tr>/i);
  if (fragmentMatch?.index !== undefined) {
    const fragment = fragmentMatch[0];
    const table = parseHtmlTableBlock(markdown, fragment, fragmentMatch.index, fragmentMatch.index + fragment.length, 0);
    return table ? [table] : [];
  }

  const cellFragmentMatch = markdown.match(/(?:<t[dh]\b[\s\S]*?<\/t[dh]>\s*)+/i);
  if (!cellFragmentMatch || cellFragmentMatch.index === undefined) return [];
  const fragment = `<tr>${cellFragmentMatch[0]}</tr>`;
  const table = parseHtmlTableBlock(markdown, fragment, cellFragmentMatch.index, cellFragmentMatch.index + cellFragmentMatch[0].length, 0);
  return table ? [table] : [];
}

function parseTables(markdown: string): ParsedTable[] {
  return [...parseMarkdownTables(markdown), ...parseHtmlTables(markdown)]
    .sort((a, b) => (a.charStart ?? 0) - (b.charStart ?? 0));
}

function escapeCell(value: string): string {
  return value.replace(/\r?\n/g, "<br>").replace(/\|/g, "\\|").trim();
}

function renderTable(table: ParsedTable, nextMeta: TableMeta = table.meta): string[] {
  const columnCount = Math.max(1, table.header.length);
  const columnWidths = normalizeNumbers(nextMeta.columnWidths, columnCount, DEFAULT_COLUMN_WIDTH);
  const rowHeights = normalizeNumbers(nextMeta.rowHeights, table.rows.length + 1, DEFAULT_ROW_HEIGHT);
  const metaLine = `${META_PREFIX} ${JSON.stringify({ columnWidths, rowHeights })} ${META_SUFFIX}`;
  const row = (cells: string[]) => `| ${padCells(cells, columnCount).map(escapeCell).join(" | ")} |`;
  return [
    metaLine,
    row(table.header),
    row(Array.from({ length: columnCount }, () => "---")),
    ...table.rows.map(row),
  ];
}

function renderHtmlTable(table: ParsedTable, nextMeta: TableMeta = table.meta): string {
  const columnCount = Math.max(1, table.header.length);
  const columnWidths = normalizeNumbers(nextMeta.columnWidths, columnCount, DEFAULT_COLUMN_WIDTH);
  const rowHeights = normalizeNumbers(nextMeta.rowHeights, table.rows.length + 1, DEFAULT_ROW_HEIGHT);
  const rows = [table.header, ...table.rows].map((row, rowIndex) => {
    const cells = padCells(row, columnCount).map((cell, columnIndex) => {
      const tag = rowIndex === 0 ? "th" : "td";
      const width = Math.round(columnWidths[columnIndex]);
      const height = Math.round(rowHeights[rowIndex] ?? DEFAULT_ROW_HEIGHT);
      return `<${tag} colwidth="${width}" style="width:${width}px;min-height:${height}px;"><p>${escapeHtml(cell).replace(/\r?\n/g, "<br>")}</p></${tag}>`;
    });
    return `<tr>${cells.join("")}</tr>`;
  });
  return `<table><tbody>${rows.join("")}</tbody></table>`;
}

function replaceTable(markdown: string, table: ParsedTable, nextTable: ParsedTable, nextMeta: TableMeta = nextTable.meta): string {
  if (table.sourceKind === "html" && table.charStart !== undefined && table.charEnd !== undefined) {
    return `${markdown.slice(0, table.charStart)}${renderHtmlTable(nextTable, nextMeta)}${markdown.slice(table.charEnd)}`;
  }
  const newline = markdown.includes("\r\n") ? "\r\n" : "\n";
  const lines = markdown.split(/\r?\n/);
  lines.splice(table.blockStart, table.blockEnd - table.blockStart, ...renderTable(nextTable, nextMeta));
  return lines.join(newline);
}

function formatMarkdownTables(markdown: string): string {
  let next = markdown;
  const tables = parseTables(markdown).reverse();
  for (const table of tables) {
    next = replaceTable(next, table, table);
  }
  return next;
}

interface ExcelMarkdownSourceEditorProps {
  content: string;
  onChange: (markdown: string) => void;
  placeholder?: string;
}

export function ExcelMarkdownSourceEditor({
  content,
  onChange,
  placeholder = "在这里直接编辑导入后的 Markdown 表格...",
}: ExcelMarkdownSourceEditorProps) {
  const tables = useMemo(() => parseTables(content), [content]);
  const [selected, setSelected] = useState<SelectedCell | null>(null);
  const [showSource, setShowSource] = useState(false);

  function mutateTable(table: ParsedTable, updater: (draft: ParsedTable) => ParsedTable, meta?: TableMeta) {
    const draft = {
      ...table,
      header: [...table.header],
      rows: table.rows.map((row) => [...row]),
      meta: { ...table.meta },
    };
    const nextTable = updater(draft);
    onChange(replaceTable(content, table, nextTable, meta ?? nextTable.meta));
  }

  function updateCell(table: ParsedTable, rowIndex: number, columnIndex: number, value: string) {
    mutateTable(table, (draft) => {
      if (rowIndex === 0) draft.header[columnIndex] = value;
      else draft.rows[rowIndex - 1][columnIndex] = value;
      return draft;
    });
    setSelected({ tableId: table.id, row: rowIndex, col: columnIndex });
  }

  function insertRow(table: ParsedTable, rowIndex: number, after: boolean) {
    mutateTable(table, (draft) => {
      const insertAt = rowIndex === 0 ? 0 : Math.max(0, rowIndex - 1 + (after ? 1 : 0));
      draft.rows.splice(insertAt, 0, Array.from({ length: draft.header.length }, () => ""));
      const rowHeights = normalizeNumbers(draft.meta.rowHeights, draft.rows.length, DEFAULT_ROW_HEIGHT);
      rowHeights.splice(insertAt + 1, 0, DEFAULT_ROW_HEIGHT);
      draft.meta.rowHeights = rowHeights;
      setSelected({ tableId: table.id, row: insertAt + 1, col: 0 });
      return draft;
    });
  }

  function deleteRow(table: ParsedTable, rowIndex: number) {
    if (rowIndex === 0) return;
    mutateTable(table, (draft) => {
      draft.rows.splice(rowIndex - 1, 1);
      const rowHeights = normalizeNumbers(draft.meta.rowHeights, table.rows.length + 1, DEFAULT_ROW_HEIGHT);
      rowHeights.splice(rowIndex, 1);
      draft.meta.rowHeights = rowHeights;
      setSelected({ tableId: table.id, row: Math.max(0, rowIndex - 1), col: 0 });
      return draft;
    });
  }

  function insertColumn(table: ParsedTable, columnIndex: number, after: boolean) {
    mutateTable(table, (draft) => {
      const insertAt = Math.max(0, columnIndex + (after ? 1 : 0));
      draft.header.splice(insertAt, 0, `列 ${insertAt + 1}`);
      draft.rows = draft.rows.map((row) => {
        const next = [...row];
        next.splice(insertAt, 0, "");
        return next;
      });
      const columnWidths = normalizeNumbers(draft.meta.columnWidths, table.header.length, DEFAULT_COLUMN_WIDTH);
      columnWidths.splice(insertAt, 0, DEFAULT_COLUMN_WIDTH);
      draft.meta.columnWidths = columnWidths;
      setSelected({ tableId: table.id, row: 0, col: insertAt });
      return draft;
    });
  }

  function deleteColumn(table: ParsedTable, columnIndex: number) {
    if (table.header.length <= 1) return;
    mutateTable(table, (draft) => {
      draft.header.splice(columnIndex, 1);
      draft.rows = draft.rows.map((row) => {
        const next = [...row];
        next.splice(columnIndex, 1);
        return next;
      });
      const columnWidths = normalizeNumbers(draft.meta.columnWidths, table.header.length, DEFAULT_COLUMN_WIDTH);
      columnWidths.splice(columnIndex, 1);
      draft.meta.columnWidths = columnWidths;
      setSelected({ tableId: table.id, row: 0, col: Math.max(0, columnIndex - 1) });
      return draft;
    });
  }

  function updateLayout(table: ParsedTable, kind: "column" | "row", index: number, value: number | null) {
    const columnWidths = normalizeNumbers(table.meta.columnWidths, table.header.length, DEFAULT_COLUMN_WIDTH);
    const rowHeights = normalizeNumbers(table.meta.rowHeights, table.rows.length + 1, DEFAULT_ROW_HEIGHT);
    if (kind === "column") columnWidths[index] = value ?? DEFAULT_COLUMN_WIDTH;
    else rowHeights[index] = value ?? DEFAULT_ROW_HEIGHT;
    onChange(replaceTable(content, table, table, { columnWidths, rowHeights }));
  }

  function activeFor(table: ParsedTable): SelectedCell {
    if (selected?.tableId === table.id) return selected;
    return { tableId: table.id, row: 0, col: 0 };
  }

  function handleFormatTables() {
    const next = formatMarkdownTables(content);
    if (next !== content) onChange(next);
  }

  return (
    <div className="mb-6">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <Space wrap>
          <Text strong>表格编辑</Text>
          <Text type="secondary">识别到 {tables.length} 个表格</Text>
        </Space>
        <Space wrap>
          <Button size="small" onClick={handleFormatTables}>
            整理 Markdown
          </Button>
          <Button
            size="small"
            icon={showSource ? <EyeOff size={14} /> : <Eye size={14} />}
            onClick={() => setShowSource((value) => !value)}
          >
            {showSource ? "隐藏源码" : "查看源码"}
          </Button>
        </Space>
      </div>

      {tables.length === 0 ? (
        <div className="rounded-lg border border-dashed border-gray-200 bg-white p-6">
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有识别到可编辑表格" />
          <Input.TextArea
            className="mt-3"
            value={content}
            onChange={(event) => onChange(event.target.value)}
            placeholder={placeholder}
            autoSize={{ minRows: 18, maxRows: 40 }}
          />
        </div>
      ) : (
        <div className="space-y-4">
          {tables.map((table, tableIndex) => {
            const active = activeFor(table);
            const selectedColumn = Math.min(active.col, Math.max(table.header.length - 1, 0));
            const selectedRow = Math.min(active.row, table.rows.length);
            const columnWidths = normalizeNumbers(table.meta.columnWidths, table.header.length, DEFAULT_COLUMN_WIDTH);
            const rowHeights = normalizeNumbers(table.meta.rowHeights, table.rows.length + 1, DEFAULT_ROW_HEIGHT);
            const grid = [table.header, ...table.rows];

            return (
              <div
                key={table.id}
                className="rounded-xl border bg-white p-3"
                style={{ borderColor: "var(--color-border-subtle, #e5e7eb)" }}
              >
                <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
                  <Space wrap>
                    <Text strong>{table.title}</Text>
                    <Text type="secondary">
                      表格 {tableIndex + 1} · {table.rows.length + 1} 行 / {table.header.length} 列
                    </Text>
                  </Space>
                  <Space wrap>
                    <Button size="small" icon={<Rows3 size={14} />} onClick={() => insertRow(table, selectedRow, false)}>
                      上方插入行
                    </Button>
                    <Button size="small" icon={<Rows3 size={14} />} onClick={() => insertRow(table, selectedRow, true)}>
                      下方插入行
                    </Button>
                    <Button size="small" danger icon={<Trash2 size={14} />} disabled={selectedRow === 0} onClick={() => deleteRow(table, selectedRow)}>
                      删除行
                    </Button>
                    <Button size="small" icon={<Columns3 size={14} />} onClick={() => insertColumn(table, selectedColumn, false)}>
                      左侧插入列
                    </Button>
                    <Button size="small" icon={<Columns3 size={14} />} onClick={() => insertColumn(table, selectedColumn, true)}>
                      右侧插入列
                    </Button>
                    <Button
                      size="small"
                      danger
                      icon={<Trash2 size={14} />}
                      disabled={table.header.length <= 1}
                      onClick={() => deleteColumn(table, selectedColumn)}
                    >
                      删除列
                    </Button>
                  </Space>
                </div>

                <div className="mb-3 flex flex-wrap items-center gap-3">
                  <Space>
                    <Text type="secondary">选中列宽</Text>
                    <InputNumber
                      min={80}
                      max={520}
                      value={columnWidths[selectedColumn]}
                      addonAfter="px"
                      onChange={(value) => updateLayout(table, "column", selectedColumn, value)}
                    />
                  </Space>
                  <Space>
                    <Text type="secondary">选中行高</Text>
                    <InputNumber
                      min={28}
                      max={180}
                      value={rowHeights[selectedRow]}
                      addonAfter="px"
                      onChange={(value) => updateLayout(table, "row", selectedRow, value)}
                    />
                  </Space>
                  <Text type="secondary">
                    当前选中：第 {selectedRow + 1} 行，第 {selectedColumn + 1} 列
                  </Text>
                </div>

                <div className="max-h-[520px] overflow-auto rounded-lg border border-gray-100">
                  <table style={{ borderCollapse: "separate", borderSpacing: 0 }}>
                    <thead>
                      <tr>
                        <th
                          style={{
                            position: "sticky",
                            top: 0,
                            left: 0,
                            zIndex: 3,
                            width: 42,
                            minWidth: 42,
                            borderRight: "1px solid #eee",
                            borderBottom: "1px solid #eee",
                            background: "#f5f1ea",
                          }}
                        />
                        {table.header.map((_, columnIndex) => {
                          const isSelected = selected?.tableId === table.id && selected.col === columnIndex;
                          return (
                            <th
                              key={columnIndex}
                              style={{
                                position: "sticky",
                                top: 0,
                                zIndex: 2,
                                width: columnWidths[columnIndex],
                                minWidth: columnWidths[columnIndex],
                                borderRight: "1px solid #eee",
                                borderBottom: "1px solid #eee",
                                background: isSelected ? "#e6f4ff" : "#f5f1ea",
                                color: "#8a5a2b",
                                fontSize: 12,
                                fontWeight: 600,
                                textAlign: "center",
                              }}
                            >
                              {columnName(columnIndex)}
                            </th>
                          );
                        })}
                      </tr>
                    </thead>
                    <tbody>
                      {grid.map((row, rowIndex) => (
                        <tr key={rowIndex} style={{ height: rowHeights[rowIndex] }}>
                          <th
                            style={{
                              position: "sticky",
                              left: 0,
                              zIndex: 1,
                              width: 42,
                              minWidth: 42,
                              borderRight: "1px solid #eee",
                              borderBottom: "1px solid #eee",
                              background: "#fafafa",
                              color: "#999",
                              fontWeight: 500,
                            }}
                          >
                            {rowIndex + 1}
                          </th>
                          {row.map((cell, columnIndex) => {
                            const isSelected = selected?.tableId === table.id && selected.row === rowIndex && selected.col === columnIndex;
                            return (
                              <td
                                key={`${rowIndex}-${columnIndex}`}
                                style={{
                                  width: columnWidths[columnIndex],
                                  minWidth: columnWidths[columnIndex],
                                  maxWidth: columnWidths[columnIndex],
                                  borderRight: "1px solid #eee",
                                  borderBottom: "1px solid #eee",
                                  background: rowIndex === 0 ? "#fbfbfb" : "#fff",
                                  outline: isSelected ? "2px solid #1677ff" : "none",
                                  outlineOffset: -2,
                                  verticalAlign: "top",
                                }}
                              >
                                <Input.TextArea
                                  value={cell}
                                  autoSize={false}
                                  onFocus={() => setSelected({ tableId: table.id, row: rowIndex, col: columnIndex })}
                                  onChange={(event) => updateCell(table, rowIndex, columnIndex, event.target.value)}
                                  style={{
                                    minHeight: Math.max(26, rowHeights[rowIndex] - 10),
                                    resize: "none",
                                    border: "none",
                                    boxShadow: "none",
                                    background: "transparent",
                                    color: "var(--color-text, #1f2937)",
                                    fontWeight: rowIndex === 0 ? 600 : 400,
                                  }}
                                />
                              </td>
                            );
                          })}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {showSource && (
        <div className="mt-4 rounded-xl border border-dashed border-gray-200 bg-white p-3">
          <div className="mb-2">
            <Text strong>Markdown 源码</Text>
            <Text type="secondary" className="ml-2">
              可用于复制、批量粘贴或手工修复表格源码。
            </Text>
          </div>
          <Input.TextArea
            value={content}
            onChange={(event) => onChange(event.target.value)}
            placeholder={placeholder}
            autoSize={{ minRows: 12, maxRows: 32 }}
            wrap="off"
            spellCheck={false}
            style={{
              fontFamily:
                'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
              fontSize: 13,
              lineHeight: 1.75,
              whiteSpace: "pre",
              overflowX: "auto",
            }}
          />
        </div>
      )}
    </div>
  );
}
