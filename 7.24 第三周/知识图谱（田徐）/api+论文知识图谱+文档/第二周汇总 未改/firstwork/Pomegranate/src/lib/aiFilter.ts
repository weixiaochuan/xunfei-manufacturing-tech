export function stripPseudoToolCalls(value: string): string {
  return value.replace(/<tool_call>[\s\S]*?<\/tool_call>/g, "").trim();
}
