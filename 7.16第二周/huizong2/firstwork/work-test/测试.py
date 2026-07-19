# -*- coding: utf-8 -*-

import json
import requests


API_KEY = "ed07434610b056e4f9fd3ae3a056141f"
API_SECRET = "M2MwOWIwZTEyZDM2OGYzMTNlYTdjOTRh"
FLOW_ID = "7483111093658759168"

url = "https://xingchen-api.xf-yun.com/workflow/v1/chat/completions"

headers = {
    "Authorization": f"Bearer {API_KEY}:{API_SECRET}",
    "Content-Type": "application/json"
}


def read_user_input():
    """
    在运行界面读取多行输入。
    输入完成后，单独输入 END 并回车。
    """
    print("=" * 60)
    print("请输入你的内容。")
    print("输入完成后，请单独输入 END，然后按回车。")
    print("=" * 60)

    lines = []

    while True:
        try:
            line = input()
        except EOFError:
            break

        if line.strip().upper() == "END":
            break

        lines.append(line)

    user_input = "\n".join(lines).strip()

    if not user_input:
        raise ValueError("输入内容不能为空。")

    return user_input


def main():
    try:
        user_input = read_user_input()

        data = {
            "flow_id": FLOW_ID,

            "uid": "test001",

            "parameters": {
                "AGENT_USER_INPUT": user_input
            },

            "ext": {
                "bot_id": "workflow",
                "caller": "workflow"
            },

            "stream": False
        }

        # 禁止 requests 自动读取系统代理等环境配置，
        # 避免代理信息中含中文时出现 latin-1 编码错误。
        session = requests.Session()
        session.trust_env = False

        response = session.post(
            url,
            headers=headers,
            json=data,
            timeout=(15, 300)
        )

        print("\n状态码:", response.status_code)
        print("\n接口原始返回:")
        print(response.text)

        result = response.json()

        if result.get("code") != 0:
            print("\n工作流执行失败")
            print("错误码:", result.get("code"))
            print("错误信息:", result.get("message"))
            return

        choices = result.get("choices") or []

        if not choices:
            print("\n接口调用成功，但没有返回 choices。")
            return

        content = (
            choices[0]
            .get("delta", {})
            .get("content", "")
        )

        print("\n工作流最终输出:")
        print(content)

    except ValueError as error:
        print("\n输入错误:", error)

    except requests.Timeout:
        print("\n请求超时。")

    except requests.ConnectionError as error:
        print("\n网络连接失败:", error)

    except json.JSONDecodeError:
        print("\n接口返回的内容不是合法 JSON。")

    except Exception as error:
        print(
            f"\n程序出现异常："
            f"{type(error).__name__}: {error}"
        )
        raise


if __name__ == "__main__":
    main()