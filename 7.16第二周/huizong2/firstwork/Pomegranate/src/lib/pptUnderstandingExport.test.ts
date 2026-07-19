import {
  buildPptUnderstandingMarkdown,
  sanitizeMarkdownExportFilename,
} from "./pptUnderstandingExport.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const exportedAt = new Date(2026, 6, 11, 10, 30);
const base = buildPptUnderstandingMarkdown({
  title: "中华传统文化",
  audience: "课程学生",
  pageCount: "8 页",
  style: "简约商务",
  generationMode: "template",
  materialInputMode: "internal",
  materialSources: [
    { id: 1, sourceType: "document", title: "课程素材" },
    { id: 2, sourceType: "diary", title: "2026-07-10 的日记" },
  ],
  understandingDraft: {
    understandingSummary: "保留中文摘要",
    keyPriorities: "1. 重点一\n2. 重点二",
    narrativeMainline: "",
    suggestedPageStructure: "P01 引入\nP02 展开",
    visualExpressionAdvice: "图文结合",
    openQuestions: "暂无补充问题",
  },
  exportedAt,
});

for (const heading of [
  "## AI 理解摘要",
  "## 重点取舍",
  "## 叙事主线",
  "## 建议页面结构",
  "## 视觉与表达建议",
  "## 仍需确认的问题",
]) {
  assert(base.content.includes(heading), `missing heading: ${heading}`);
}
assert(base.content.includes("1. 重点一\n2. 重点二"), "numbered lines must be preserved");
assert(base.content.includes("P01 引入\nP02 展开"), "page structure line breaks must be preserved");
assert(base.content.includes("## 叙事主线\n\n暂无"), "empty field must render 暂无");
assert(base.content.includes("- 文档｜课程素材"), "document source metadata missing");
assert(base.content.includes("- 日记｜2026-07-10 的日记"), "diary source metadata missing");
assert(!base.content.includes("rawMaterial"), "raw material must not be exported");
assert(base.content.includes("保留中文摘要"), "Chinese content must remain unchanged");

const invalidFilename = sanitizeMarkdownExportFilename(' 标题\\/:*?"<>|. ', exportedAt);
assert(!/[\\/:*?"<>|]/.test(invalidFilename), "Windows-invalid filename characters remain");
assert(
  sanitizeMarkdownExportFilename("", exportedAt) === "PPT需求理解_20260711_1030.md",
  "empty title filename is incorrect",
);
assert(
  sanitizeMarkdownExportFilename("很长".repeat(80), exportedAt).length < 120,
  "long filename was not truncated",
);

const stale = buildPptUnderstandingMarkdown({
  ...({
    title: "测试",
    audience: "暂无",
    pageCount: "4 页",
    style: "学术汇报",
    generationMode: "template" as const,
    materialInputMode: "manual" as const,
    materialSources: [],
    understandingDraft: {
      understandingSummary: "摘要",
      keyPriorities: "重点",
      narrativeMainline: "主线",
      suggestedPageStructure: "结构",
      visualExpressionAdvice: "视觉",
      openQuestions: "问题",
    },
    exportedAt,
  }),
  stale: true,
});
assert(stale.content.includes("当前素材已发生变化"), "stale warning missing");
assert(stale.content.includes("- 直接输入的文字材料"), "manual source label missing");
