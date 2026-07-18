import { buildAiUnderstandingPrompt } from "./pptUnderstandingPrompt.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const prompt = buildAiUnderstandingPrompt({
  topic: "人物生平",
  sourceMaterial: "1893年出生，1976年逝世。",
  audience: "大学课堂",
  pageCount: "6 页",
  style: "学术简洁",
  extraRequirements: "保持中立",
});

for (const required of [
  "每一项必须单独占一行",
  "叙事主线包含多个阶段时也要分行",
  "页面结构、内容比例和版式设计由你直接完成",
  "素材缺失但生成依赖的事实",
  "互相冲突的数据取舍",
  "汇报立场",
  "敏感个人信息",
  "不得询问页面怎么排版",
  "信息层级",
  "视觉平衡",
  "暂无需要用户补充的信息，系统将根据现有材料自动完成内容组织与版式规划。",
]) {
  assert(prompt.includes(required), `短素材最终 Prompt 缺少语义或格式规则：${required}`);
}
assert(prompt.includes("1893年出生，1976年逝世"), "短素材 Prompt 必须保持原始素材原文");
