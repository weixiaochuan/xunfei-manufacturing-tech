# -*- coding: utf-8 -*-

"""
功能：
1. 输入学习计划文本
2. 调用讯飞星火工作流
3. 获取工作流代码节点返回的 file_content
4. 将 Base64 内容解码并保存为 Word 文件

安装依赖：
    pip install requests

运行：
    python 星火工作流调用并保存Word.py

输入完成后，在新的一行单独输入：
    END
"""

from __future__ import annotations

import base64
import json
import re
import sys
import uuid
from datetime import datetime
from pathlib import Path
from typing import Any

import requests


# ============================================================
# 1. 你的星火平台认证信息
# ============================================================

APPID = "eab40b6c"

API_KEY = "ed07434610b056e4f9fd3ae3a056141f"

API_SECRET = "M2MwOWIwZTEyZDM2OGYzMTNlYTdjOTRh"

FLOW_ID = "7483074162653544448"

API_URL = "https://xingchen-api.xf-yun.com/workflow/v1/chat/completions"


# 工作流“输入节点”的参数名称。
# 你的星火工作流开始节点要求参数名为 AGENT_USER_INPUT。
# 该名称必须与平台开始节点的必填字段完全一致。
INPUT_PARAMETER_NAME = "AGENT_USER_INPUT"


# ============================================================
# 2. 读取用户输入
# ============================================================

def read_study_plan() -> str:
    """读取多行学习计划，单独输入 END 后结束。"""

    print("=" * 60)
    print("请输入或粘贴学习计划。")
    print("输入完成后，在新的一行单独输入 END，然后按回车。")
    print("=" * 60)

    lines: list[str] = []

    while True:
        try:
            line = input()
        except EOFError:
            break

        if line.strip().upper() == "END":
            break

        lines.append(line)

    study_plan = "\n".join(lines).strip()

    if not study_plan:
        raise ValueError("学习计划不能为空。")

    return study_plan


# ============================================================
# 3. 调用星火工作流
# ============================================================

def call_xinghuo_workflow(study_plan: str) -> Any:
    """调用星火工作流并返回 choices[0].delta.content。"""

    headers = {
        "Authorization": f"Bearer {API_KEY}:{API_SECRET}",
        "Content-Type": "application/json",
        "Accept": "application/json",
    }

    request_body = {
        "flow_id": FLOW_ID,
        "uid": f"dfjsp-{uuid.uuid4().hex[:16]}",
        "parameters": {
            INPUT_PARAMETER_NAME: study_plan
        },
        "stream": False,
    }

    print("\n正在调用星火工作流……")

    try:
        response = requests.post(
            API_URL,
            headers=headers,
            json=request_body,
            timeout=(15, 300),
        )
    except requests.Timeout as exc:
        raise RuntimeError("调用星火接口超时，请检查网络后重试。") from exc
    except requests.ConnectionError as exc:
        raise RuntimeError("无法连接星火接口，请检查网络连接。") from exc
    except requests.RequestException as exc:
        raise RuntimeError(f"接口请求失败：{exc}") from exc

    if response.status_code != 200:
        raise RuntimeError(
            f"HTTP 状态码异常：{response.status_code}\n"
            f"服务器返回：{response.text}"
        )

    try:
        response_data = response.json()
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            "星火接口没有返回合法 JSON：\n"
            f"{response.text}"
        ) from exc

    if response_data.get("code") != 0:
        raise RuntimeError(
            "星火工作流执行失败：\n"
            f"错误码：{response_data.get('code')}\n"
            f"错误信息：{response_data.get('message')}\n"
            f"完整返回：{json.dumps(response_data, ensure_ascii=False, indent=2)}"
        )

    choices = response_data.get("choices") or []

    if not choices:
        raise RuntimeError(
            "接口返回成功，但 choices 为空。\n"
            "请检查工作流输出节点是否返回了代码节点结果。\n"
            f"完整返回：{json.dumps(response_data, ensure_ascii=False, indent=2)}"
        )

    delta = choices[0].get("delta") or {}
    content = delta.get("content")

    if content is None:
        raise RuntimeError(
            "接口返回成功，但 choices[0].delta.content 不存在。\n"
            f"完整返回：{json.dumps(response_data, ensure_ascii=False, indent=2)}"
        )

    return content


# ============================================================
# 4. 解析工作流输出
# ============================================================

def strip_json_code_block(text: str) -> str:
    """移除可能存在的 ```json ... ``` 包裹。"""

    text = text.strip()

    match = re.fullmatch(
        r"```(?:json)?\s*(.*?)\s*```",
        text,
        flags=re.IGNORECASE | re.DOTALL,
    )

    if match:
        return match.group(1).strip()

    return text


def parse_workflow_result(content: Any) -> dict[str, Any]:
    """
    将工作流输出转换为字典。

    支持以下情况：
    1. content 本身就是字典
    2. content 是 JSON 字符串
    3. content 被 ```json ``` 包裹
    4. JSON 被再次字符串化
    """

    current = content

    for _ in range(3):
        if isinstance(current, dict):
            return current

        if not isinstance(current, str):
            break

        current = strip_json_code_block(current)

        try:
            current = json.loads(current)
        except json.JSONDecodeError:
            break

    raise RuntimeError(
        "无法从工作流输出中解析 file_content。\n"
        "请确认输出节点返回的是代码节点的完整结果，例如：\n"
        '{"file_content":"UEsDB...","file_name":"学习计划.docx"}\n\n'
        f"实际输出：\n{content}"
    )


# ============================================================
# 5. Base64 解码并保存 Word
# ============================================================

def safe_file_name(file_name: str) -> str:
    """移除文件名中的非法字符。"""

    cleaned = re.sub(r'[\\/:*?"<>|]', "_", file_name).strip()

    if not cleaned:
        cleaned = "学习计划.docx"

    if not cleaned.lower().endswith(".docx"):
        cleaned += ".docx"

    return cleaned


def save_word_file(result: dict[str, Any]) -> Path:
    """从工作流返回值中读取 Base64，并保存为 Word 文件。"""

    file_content = result.get("file_content")

    if not file_content:
        raise RuntimeError(
            "工作流输出中没有 file_content。\n"
            f"实际输出字段：{list(result.keys())}"
        )

    if not isinstance(file_content, str):
        raise RuntimeError(
            "file_content 不是字符串，无法进行 Base64 解码。"
        )

    requested_name = str(
        result.get("file_name") or "学习计划.docx"
    )

    file_name = safe_file_name(requested_name)

    # 为避免覆盖旧文件，在文件名后增加时间。
    stem = Path(file_name).stem
    suffix = Path(file_name).suffix or ".docx"
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    final_name = f"{stem}_{timestamp}{suffix}"

    output_path = Path.cwd() / final_name

    # 兼容 data:application/...;base64,xxxx 这种形式
    if "," in file_content and ";base64" in file_content[:100]:
        file_content = file_content.split(",", 1)[1]

    # 去除 Base64 中可能出现的空格和换行
    normalized_base64 = re.sub(r"\s+", "", file_content)

    try:
        word_bytes = base64.b64decode(
            normalized_base64,
            validate=True,
        )
    except Exception as exc:
        raise RuntimeError(
            "file_content 不是合法的 Base64 内容，无法生成 Word 文件。"
        ) from exc

    # docx 本质是 ZIP 文件，正常文件通常以 PK 开头。
    if not word_bytes.startswith(b"PK"):
        raise RuntimeError(
            "Base64 已解码，但内容不像有效的 docx 文件。\n"
            "请检查星火代码节点是否确实执行了 doc.save(file_stream)。"
        )

    output_path.write_bytes(word_bytes)

    return output_path.resolve()


# ============================================================
# 6. 主程序
# ============================================================

def main() -> None:
    try:
        study_plan = read_study_plan()

        content = call_xinghuo_workflow(study_plan)

        print("\n工作流调用成功，正在解析文件内容……")

        result = parse_workflow_result(content)

        # 如果代码节点同时返回了原始文本，也可以在控制台显示
        text = result.get("text")

        if text:
            print("\n工作流返回的学习计划文本：")
            print("=" * 60)
            print(text)
            print("=" * 60)

        output_path = save_word_file(result)

        print("\nWord 文件生成成功：")
        print(output_path)

    except (ValueError, RuntimeError) as exc:
        print(f"\n程序执行失败：\n{exc}")
        sys.exit(1)

    except KeyboardInterrupt:
        print("\n程序已取消。")
        sys.exit(130)

    except Exception as exc:
        print(
            f"\n发生未预期错误："
            f"{type(exc).__name__}: {exc}"
        )
        sys.exit(1)


if __name__ == "__main__":
    main()