"""
质检校验模块 —— 概念题和计算题走两条独立校验逻辑（报告结论：
"不能与概念题共用审核流程"）。

计算题验算说明（诚实说明能力边界，别指望它是万能验算器）：
- 我们对模型返回的 calculation_steps 里每一行做正则抽取，找形如
  "... = 数字" 的算式片段，用受限的安全eval重新算一遍，跟模型自己
  写的结果比对。
- 能抽取到算式且数值对不上 -> 判定"验算失败"，打回重生成。
- 抽取不到任何可核算的算式（比如模型只写了文字描述没写算式）
  -> 判定"无法自动验算-需人工复核"，不是直接判过，也不是直接判失败，
     交给人工审核环节兜底。这是一个可以随着实际生成样本持续调优的地方，
     后续可以逐步把正则换成更严格的算式解析器，或者接一个专门做验算的
     模型调用（报告里提到的"代码验算脚本"可以从这里往下细化）。
"""
import ast
import operator
import re

REQUIRED_CONCEPT_FIELDS = ["stem", "options", "answer", "explanation"]
REQUIRED_CALC_FIELDS = ["stem", "calculation_steps", "answer", "explanation"]

_ALLOWED_OPERATORS = {
    ast.Add: operator.add,
    ast.Sub: operator.sub,
    ast.Mult: operator.mul,
    ast.Div: operator.truediv,
    ast.USub: operator.neg,
    ast.UAdd: operator.pos,
    ast.Pow: operator.pow,
}


def _safe_eval(expr: str):
    """只允许数字和 + - * / ( ) ** 的受限求值，不能执行任意代码。"""
    node = ast.parse(expr, mode="eval").body
    return _eval_node(node)


def _eval_node(node):
    if isinstance(node, ast.Constant):
        if isinstance(node.value, (int, float)):
            return node.value
        raise ValueError("非数字常量")
    if isinstance(node, ast.BinOp) and type(node.op) in _ALLOWED_OPERATORS:
        return _ALLOWED_OPERATORS[type(node.op)](_eval_node(node.left), _eval_node(node.right))
    if isinstance(node, ast.UnaryOp) and type(node.op) in _ALLOWED_OPERATORS:
        return _ALLOWED_OPERATORS[type(node.op)](_eval_node(node.operand))
    raise ValueError("不支持的表达式")


# 匹配形如 "3.5 + 0.8 - 1 = 3.3" 这样的片段（等号右边是数字，左边是纯算式）
_EXPR_PATTERN = re.compile(
    r"([0-9]+(?:\.[0-9]+)?(?:\s*[-+*/]\s*[0-9]+(?:\.[0-9]+)?)+)\s*=\s*(-?[0-9]+(?:\.[0-9]+)?)"
)


def validate_concept_question(question: dict) -> dict:
    """返回 {"passed": bool, "errors": [...]}"""
    errors = []
    for field in REQUIRED_CONCEPT_FIELDS:
        if not question.get(field):
            errors.append(f"缺少必填字段: {field}")

    options = question.get("options") or []
    if len(options) < 2:
        errors.append("选项数量过少（至少需要2个）")

    correct_options = [o for o in options if o.get("is_correct")]
    if len(correct_options) != 1:
        errors.append(f"正确选项数量应为1个，实际检测到{len(correct_options)}个")

    if correct_options and question.get("answer") != correct_options[0].get("text"):
        errors.append("answer字段与标记为is_correct的选项文本不一致")

    incorrect_without_misconception = [
        o for o in options if not o.get("is_correct") and not o.get("misconception")
    ]
    if incorrect_without_misconception:
        errors.append(
            f"{len(incorrect_without_misconception)}个干扰项未标注对应的认知误区（misconception字段为空）"
        )

    # 首轮审核发现："4个选项文字一模一样"这种结构性废题。这里检测重复选项文本。
    texts = [str(o.get("text", "")).strip() for o in options if o.get("text")]
    if len(texts) != len(set(texts)):
        errors.append("存在文字完全相同的选项（选项必须互不相同）")

    return {"passed": len(errors) == 0, "errors": errors}


def validate_computation_question(question: dict) -> dict:
    """返回 {"passed": bool, "calc_status": str, "detail": str, "errors": [...]}
    calc_status: 已验算通过 / 验算失败 / 无法自动验算-需人工复核
    """
    errors = []
    for field in REQUIRED_CALC_FIELDS:
        if not question.get(field):
            errors.append(f"缺少必填字段: {field}")

    steps = question.get("calculation_steps") or []
    if not isinstance(steps, list) or len(steps) == 0:
        errors.append("calculation_steps为空或格式不是列表")
        return {"passed": False, "calc_status": "验算失败", "detail": "无计算步骤", "errors": errors}

    checked, mismatches = 0, []
    for step in steps:
        for expr, claimed in _EXPR_PATTERN.findall(str(step)):
            checked += 1
            try:
                computed = _safe_eval(expr)
                if abs(computed - float(claimed)) > 1e-3:
                    mismatches.append(f"「{expr.strip()}」应为{computed:.4g}，模型写的是{claimed}")
            except Exception as e:  # noqa: BLE001
                mismatches.append(f"「{expr}」表达式解析失败: {e}")

    if checked == 0:
        calc_status = "无法自动验算-需人工复核"
        detail = "未能从calculation_steps中抽取到可核算的算式，转人工审核"
    elif mismatches:
        calc_status = "验算失败"
        detail = "; ".join(mismatches)
        errors.append("计算步骤自洽性验算未通过")
    else:
        calc_status = "已验算通过"
        detail = f"共核对{checked}处算式，均一致"

    return {
        "passed": len(errors) == 0,
        "calc_status": calc_status,
        "detail": detail,
        "errors": errors,
    }
