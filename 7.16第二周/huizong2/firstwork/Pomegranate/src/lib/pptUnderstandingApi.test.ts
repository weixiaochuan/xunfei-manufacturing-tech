import { readFileSync } from "node:fs";
import { aiWriteApi } from "./api.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const api = aiWriteApi as unknown as Record<string, unknown>;
assert(typeof api.understandPpt === "function", "短素材必须保留单次最终六维理解 API");
assert(typeof api.understandPptChunk === "function", "长素材每段必须直接调用六维草稿 API");
assert(typeof api.mergePptUnderstanding === "function", "长素材必须保留唯一最终合并 API");
const apiSource = readFileSync(new URL("./api.ts", import.meta.url), "utf8");
assert(apiSource.includes('requestKind: "direct"'), "短素材请求必须标记为 direct");
assert(apiSource.includes('requestKind: "chunk"'), "分段理解请求必须标记为 chunk");
assert(apiSource.includes('requestKind: "merge"'), "分段合并请求必须标记为 merge");
assert(!("cleanPptMaterial" in api), "AI 清洗请求必须彻底删除");
assert(!("analyzePptMaterialChunk" in api), "旧通用素材分析请求必须彻底删除");
assert(!("organizePptMaterialMap" in api), "素材地图与全局组织请求必须彻底删除");
