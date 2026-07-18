import { aiWriteApi } from "./api/index.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const api = aiWriteApi as unknown as Record<string, unknown>;
assert(typeof api.understandPpt === "function", "短素材必须保留单次最终六维理解 API");
assert(typeof api.understandPptChunk === "function", "长素材每段必须直接调用六维草稿 API");
assert(typeof api.mergePptUnderstanding === "function", "长素材必须保留唯一最终合并 API");
assert(!("cleanPptMaterial" in api), "AI 清洗请求必须彻底删除");
assert(!("analyzePptMaterialChunk" in api), "旧通用素材分析请求必须彻底删除");
assert(!("organizePptMaterialMap" in api), "素材地图与全局组织请求必须彻底删除");
