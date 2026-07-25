"""
模型无关的 LLM 客户端（升级版：支持多模型注册 + 运行时自选）。

设计要点
--------
1. 换模型只改 config/llm_config.json，不动业务代码。
2. config 里 providers 是一个"模型注册表"，你想登记几个就登记几个
   （DeepSeek / 讯飞星火 / Kimi / 智谱GLM / 本地 …），国内主流模型基本都
   提供 OpenAI 兼容的 /chat/completions 接口，所以一个通用客户端全兼容。
3. roles 决定默认："概念题用哪个、计算题用哪个"。
   跑命令时加 --provider 名字，可临时把这一次全换成某个模型——方便你
   拿同一批知识点分别用几个模型各跑一遍做对比。
4. 沙箱/无网络时用 provider=mock，不联网、不花钱，只验证工程链路。
"""
import json
import os

import requests

BASE_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LOCAL_CONFIG_PATH = os.path.join(BASE_DIR, "config", "llm_config.json")
EXAMPLE_CONFIG_PATH = os.path.join(BASE_DIR, "config", "llm_config.example.json")
CONFIG_PATH = os.environ.get("QUESTION_BANK_LLM_CONFIG", LOCAL_CONFIG_PATH)


def load_config():
    path = CONFIG_PATH if os.path.exists(CONFIG_PATH) else EXAMPLE_CONFIG_PATH
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


class OpenAICompatibleProvider:
    """适配任何提供 OpenAI 兼容 /chat/completions 接口的厂商。"""

    def __init__(self, name, base_url, api_key, model, timeout=90):
        self.name = name
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.model = model
        self.timeout = timeout

    @property
    def label(self):
        # 记录到题库 generation_model 字段，审核时能看出这道题是哪个模型出的
        return f"{self.name}/{self.model}"

    def chat(self, system_prompt, user_prompt, temperature=0.7, response_json=True):
        key = (self.api_key or "").strip()
        if not key or key.startswith("填入") or key.upper().startswith("YOUR_"):
            raise RuntimeError(
                f"provider「{self.name}」的 api_key 还没填。请到 config/llm_config.json 里填好真实 Key。"
            )
        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
        }
        payload = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
            "temperature": temperature,
        }
        if response_json:
            # 大多数厂商支持强制 JSON 输出；不支持的会忽略该字段，
            # 靠 Prompt 里的格式要求兜底，解析时仍做容错。
            payload["response_format"] = {"type": "json_object"}

        resp = requests.post(
            f"{self.base_url}/chat/completions",
            headers=headers, json=payload, timeout=self.timeout,
        )
        resp.raise_for_status()
        data = resp.json()
        return data["choices"][0]["message"]["content"]


class MockProvider:
    """离线演示：不联网，拼一个格式正确、内容是占位符的题目，验证流水线跑通。"""

    name = "mock"
    label = "mock/demo"

    def chat(self, system_prompt, user_prompt, temperature=0.7, response_json=True):
        import random
        is_calc = "出一道计算题" in system_prompt
        title = "该知识点"
        for line in user_prompt.splitlines():
            if line.startswith("知识点标题："):
                title = line.replace("知识点标题：", "").strip()
                break
        if is_calc:
            result = {
                "stem": f"【示例-待替换为真实模型输出】关于「{title}」的一道计算题题干……",
                "calculation_steps": ["Z0 = Z1 + Z2 = 0.5 + 0.3", "Z0 = 0.8"],
                "answer": "0.8",
                "explanation": f"依据「{title}」相关公式计算得出。",
                "bloom_level": "应用",
            }
        else:
            result = {
                "stem": f"【示例-待替换为真实模型输出】关于「{title}」，下列说法正确的是：",
                "options": [
                    {"text": "正确表述（示例）", "is_correct": True, "misconception": None},
                    {"text": "错误表述A（示例）", "is_correct": False, "misconception": "示例认知误区A"},
                    {"text": "错误表述B（示例）", "is_correct": False, "misconception": "示例认知误区B"},
                    {"text": "错误表述C（示例）", "is_correct": False, "misconception": "示例认知误区C"},
                ],
                "answer": "正确表述（示例）",
                "explanation": f"依据「{title}」的原文内容解释。",
                "bloom_level": random.choice(["理解", "应用", "分析"]),
            }
        return json.dumps(result, ensure_ascii=False)


def _build_provider(name, spec):
    ptype = spec.get("provider", "mock")
    if ptype == "mock":
        return MockProvider()
    if ptype == "openai_compatible":
        return OpenAICompatibleProvider(
            name=name,
            base_url=spec["base_url"],
            api_key=spec["api_key"],
            model=spec["model"],
        )
    raise ValueError(f"provider「{name}」的类型未知: {ptype}")


def get_client(role="concept", provider_name=None):
    """
    role: 'concept' 或 'computation'（决定默认用哪个已登记模型）
    provider_name: 若指定（来自命令行 --provider），则忽略 role 绑定，
                   本次直接用这个已登记的模型。
    """
    config = load_config()
    providers = config.get("providers", {})

    if provider_name:  # 命令行显式指定，优先
        chosen = provider_name
    else:
        chosen = config.get("roles", {}).get(role, "mock")

    if chosen not in providers:
        raise ValueError(
            f"配置里没有登记名为「{chosen}」的模型。可选：{list(providers.keys())}。"
            f"请在 config/llm_config.json 的 providers 里添加，或用已存在的名字。"
        )
    return _build_provider(chosen, providers[chosen])
