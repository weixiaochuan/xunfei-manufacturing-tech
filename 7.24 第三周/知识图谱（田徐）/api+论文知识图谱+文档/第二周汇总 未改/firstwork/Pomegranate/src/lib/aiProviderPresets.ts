export interface AiProviderPreset {
  provider: string;
  label: string;
  apiUrl: string;
  modelId: string;
}

export interface ModelPresetOption {
  value: string;
  label: string;
}

export const AI_PROVIDER_PRESETS: AiProviderPreset[] = [];
export const PROVIDER_PRESETS = AI_PROVIDER_PRESETS;

export const PROVIDERS: Array<{ value: string; label: string }> = [
  { value: "ollama", label: "Ollama (本地)" },
  { value: "lmstudio", label: "LM Studio (本地 OpenAI 兼容)" },
  { value: "openai", label: "OpenAI" },
  { value: "deepseek", label: "DeepSeek" },
  { value: "zhipu", label: "智谱 AI (GLM)" },
  { value: "claude", label: "Claude (经 OpenRouter 等代理)" },
  { value: "minimax", label: "Minimax" },
  { value: "siliconflow", label: "SiliconFlow (硅基流动)" },
  { value: "custom", label: "自定义 (OpenAI 兼容)" },
];

export const DEFAULT_URLS: Record<string, string> = {
  ollama: "http://localhost:11434",
  lmstudio: "http://localhost:1234/v1",
  openai: "https://api.openai.com/v1",
  deepseek: "https://api.deepseek.com",
  zhipu: "https://open.bigmodel.cn/api/paas/v4",
  claude: "https://openrouter.ai/api/v1",
  minimax: "https://api.minimax.chat/v1",
  siliconflow: "https://api.siliconflow.cn/v1",
  custom: "",
};

export const MODEL_ID_PLACEHOLDERS: Record<string, string> = {
  ollama: "如: qwen2.5:7b / llama3.2:3b",
  lmstudio: "看 LM Studio 模型页右上角 Model 标识",
  openai: "如: gpt-4o-mini / gpt-4o",
  deepseek: "如: deepseek-v4-flash / deepseek-v4-pro",
  zhipu: "如: glm-4-plus / glm-4-flash / glm-4-air",
  claude: "如: anthropic/claude-sonnet-4.6 (经 OpenRouter 等兼容代理)",
  minimax: "如: abab6.5s-chat / MiniMax-M1",
  siliconflow: "如: Qwen/Qwen2.5-72B-Instruct / deepseek-ai/DeepSeek-V3",
  custom: "填你目标服务的模型标识",
};

export const MODEL_PRESETS: Record<string, ModelPresetOption[]> = {
  ollama: [
    { value: "qwen3:4b", label: "qwen3:4b (千问3 / 入门)" },
    { value: "qwen3:8b", label: "qwen3:8b (千问3 / 推荐)" },
    { value: "qwen3:14b", label: "qwen3:14b (千问3 / 进阶)" },
    { value: "qwen3:32b", label: "qwen3:32b (千问3 / 旗舰)" },
    { value: "qwen3:30b-a3b", label: "qwen3:30b-a3b (千问3 / MoE)" },
    { value: "qwq:32b", label: "qwq:32b (千问推理)" },
    { value: "qwen2.5:7b", label: "qwen2.5:7b" },
    { value: "qwen2.5:14b", label: "qwen2.5:14b" },
    { value: "qwen2.5:32b", label: "qwen2.5:32b" },
    { value: "qwen2.5:72b", label: "qwen2.5:72b" },
    { value: "qwen2.5-coder:7b", label: "qwen2.5-coder:7b (编程)" },
    { value: "qwen2.5-coder:14b", label: "qwen2.5-coder:14b (编程)" },
    { value: "qwen2.5-coder:32b", label: "qwen2.5-coder:32b (编程)" },
    { value: "llama3.1:8b", label: "llama3.1:8b" },
    { value: "gemma2:9b", label: "gemma2:9b" },
  ],
  lmstudio: [],
  openai: [
    { value: "gpt-4o", label: "gpt-4o" },
    { value: "gpt-4o-mini", label: "gpt-4o-mini" },
    { value: "gpt-4-turbo", label: "gpt-4-turbo" },
    { value: "gpt-3.5-turbo", label: "gpt-3.5-turbo" },
    { value: "o1-mini", label: "o1-mini" },
    { value: "o1-preview", label: "o1-preview" },
  ],
  deepseek: [
    { value: "deepseek-v4-flash", label: "deepseek-v4-flash (快速 / 默认)" },
    { value: "deepseek-v4-pro", label: "deepseek-v4-pro (高质量推理)" },
  ],
  zhipu: [
    { value: "glm-4-plus", label: "glm-4-plus (旗舰)" },
    { value: "glm-4-0520", label: "glm-4-0520" },
    { value: "glm-4-air", label: "glm-4-air (轻量)" },
    { value: "glm-4-airx", label: "glm-4-airx" },
    { value: "glm-4-flash", label: "glm-4-flash (免费)" },
    { value: "glm-4-long", label: "glm-4-long (长上下文)" },
  ],
  claude: [
    { value: "anthropic/claude-sonnet-4.6", label: "anthropic/claude-sonnet-4.6 (OpenRouter)" },
    { value: "anthropic/claude-opus-4.7", label: "anthropic/claude-opus-4.7 (OpenRouter)" },
    { value: "claude-sonnet-4-5-20250929", label: "claude-sonnet-4-5-20250929" },
    { value: "claude-haiku-4-5-20251001", label: "claude-haiku-4-5-20251001" },
  ],
  minimax: [
    { value: "abab6.5s-chat", label: "abab6.5s-chat (高速)" },
    { value: "abab6.5-chat", label: "abab6.5-chat" },
    { value: "MiniMax-M1", label: "MiniMax-M1" },
  ],
  siliconflow: [
    { value: "Qwen/Qwen2.5-72B-Instruct", label: "Qwen/Qwen2.5-72B-Instruct" },
    { value: "Qwen/Qwen2.5-Coder-32B-Instruct", label: "Qwen/Qwen2.5-Coder-32B-Instruct" },
    { value: "deepseek-ai/DeepSeek-V3", label: "deepseek-ai/DeepSeek-V3" },
    { value: "deepseek-ai/DeepSeek-R1", label: "deepseek-ai/DeepSeek-R1 (推理)" },
    { value: "Pro/THUDM/glm-4-9b-chat", label: "GLM-4-9B-Chat (Pro)" },
  ],
  custom: [],
};

export const PROVIDER_NAME_MAP: Record<string, string> = Object.fromEntries(
  PROVIDERS.map((item) => [item.value, item.label]),
);

export const DEFAULT_MAX_CONTEXT = 128000;
