"""
按题目参数用代码画工程图（这是"图片准确性怎么保证"的答案）。

思路：像"孔与心轴的间隙配合""V形块定位""公差带"这类图，本质是**规则图形**，
完全可以由题目里的数值直接画出来。图是算出来的，不是模型想象出来的，
所以图和题目数据永远一致，不会出现"图上写30.021，题里是30.021 吗？"这种问题。

这比两条替代路线都可靠：
  - AI 文生图：会把尺寸、投影关系画错，工程课不能用。
  - 手工画图：慢，且改一个数就要重画。

目前实现了 3 类最常用的图（覆盖了题库里最需要图的那批题）：
  1. clearance_fit  孔/轴间隙配合示意（含最大/最小间隙、径向跳动）
  2. v_block        V形块定位与受力示意
  3. tolerance_zone 公差带示意图

用法：
    # 画一张孔轴配合图（数值来自题目）
    python3 pipeline/make_figures.py --type clearance_fit \
        --params '{"hole":"Φ30H7","hole_up":0.021,"hole_low":0,"shaft":"Φ30g6","shaft_up":-0.007,"shaft_low":-0.020,"nominal":30}' \
        --out assets/images/ch3/mandrel_fit.png

    # 画 V 形块
    python3 pipeline/make_figures.py --type v_block \
        --params '{"D":50,"alpha":90}' --out assets/images/ch3/v_block.png

    # 画公差带
    python3 pipeline/make_figures.py --type tolerance_zone \
        --params '{"nominal":30,"items":[{"name":"孔 H7","up":0.021,"low":0},{"name":"轴 g6","up":-0.007,"low":-0.020}]}' \
        --out assets/images/ch3/tz.png

画完用 db/add_image_support.py --bind 把图登记到题目上即可。
"""
import argparse
import json
import os

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Rectangle, Circle
import numpy as np

# 中文字体：容器里可能没有中文字体，退化为英文标注也不影响图形正确性
for f in ["Noto Sans CJK JP", "Noto Sans CJK SC", "WenQuanYi Zen Hei", "SimHei", "DejaVu Sans"]:
    try:
        matplotlib.rcParams["font.sans-serif"] = [f]
        break
    except Exception:
        continue
matplotlib.rcParams["axes.unicode_minus"] = False

INK = "#1a2233"
BRAND = "#2d5bd7"
BAD = "#c0392b"
OK = "#137a4b"
GREY = "#9aa7c7"


def clearance_fit(p, out):
    """孔与心轴间隙配合示意：画出孔、轴、最大/最小间隙、可能的偏移。"""
    nom = float(p.get("nominal", 30))
    hu, hl = float(p["hole_up"]), float(p["hole_low"])
    su, sl = float(p["shaft_up"]), float(p["shaft_low"])
    Dmax, Dmin = nom + hu, nom + hl
    dmax, dmin = nom + su, nom + sl
    xmax = Dmax - dmin      # 最大间隙
    xmin = Dmin - dmax      # 最小间隙
    runout = xmax / 2

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(10, 4.6))

    # 左：截面示意（孔 + 轴，轴偏到极限位置）
    scale = 1.0
    hole_r = Dmax / 2 * scale
    shaft_r = dmin / 2 * scale
    offset = runout * 40   # 放大偏移以便看清（图上注明放大）
    ax1.add_patch(Circle((0, 0), hole_r, fill=False, lw=2, ec=INK))
    ax1.add_patch(Circle((offset, 0), shaft_r, fill=True, fc="#dde5f7", ec=BRAND, lw=2))
    ax1.plot([0], [0], "+", color=INK, ms=10)
    ax1.plot([offset], [0], "+", color=BRAND, ms=10)
    ax1.annotate("", xy=(offset, 0), xytext=(0, 0),
                 arrowprops=dict(arrowstyle="->", color=BAD, lw=1.6))
    ax1.text(offset / 2, hole_r * 0.12, f"偏移 e = {runout:.4f} mm\n(= 最大间隙/2)",
             color=BAD, ha="center", fontsize=9)
    lim = hole_r * 1.35
    ax1.set_xlim(-lim, lim); ax1.set_ylim(-lim, lim)
    ax1.set_aspect("equal"); ax1.axis("off")
    ax1.set_title(f"孔 {p.get('hole','孔')} 与心轴 {p.get('shaft','轴')} 间隙配合\n"
                  f"(偏移已放大显示)", fontsize=10, color=INK)

    # 右：尺寸/间隙数值条
    ax2.axis("off")
    rows = [
        ("孔 最大极限尺寸 D_max", f"{Dmax:.3f} mm", INK),
        ("孔 最小极限尺寸 D_min", f"{Dmin:.3f} mm", INK),
        ("轴 最大极限尺寸 d_max", f"{dmax:.3f} mm", INK),
        ("轴 最小极限尺寸 d_min", f"{dmin:.3f} mm", INK),
        ("最大间隙 X_max = D_max − d_min", f"{xmax:.4f} mm", BAD),
        ("最小间隙 X_min = D_min − d_max", f"{xmin:.4f} mm", OK),
        ("最大径向跳动 Δ = X_max / 2", f"{runout:.4f} mm", BRAND),
    ]
    y = 0.92
    for name, val, c in rows:
        ax2.text(0.02, y, name, fontsize=9.5, color=INK, va="center")
        ax2.text(0.98, y, val, fontsize=9.5, color=c, va="center", ha="right", weight="bold")
        ax2.plot([0.02, 0.98], [y - 0.045, y - 0.045], color="#e6e9ef", lw=0.8)
        y -= 0.115
    ax2.set_title("由题目参数直接计算得出", fontsize=10, color=GREY)

    plt.tight_layout()
    _save(fig, out)


def v_block(p, out):
    """V形块定位与受力示意。"""
    D = float(p.get("D", 50))
    alpha = float(p.get("alpha", 90))
    r = D / 2
    half = np.deg2rad(alpha / 2)

    fig, ax = plt.subplots(figsize=(6.4, 5.2))
    # V 形块两斜面
    L = r * 2.2
    ax.plot([0, -L * np.sin(half)], [0, L * np.cos(half)], color=INK, lw=3)
    ax.plot([0, L * np.sin(half)], [0, L * np.cos(half)], color=INK, lw=3)
    # 工件圆：圆心在对称面上，到两斜面距离 = r
    cy = r / np.sin(half)
    ax.add_patch(Circle((0, cy), r, fill=True, fc="#dde5f7", ec=BRAND, lw=2))
    ax.plot([0], [cy], "+", color=BRAND, ms=10)
    # 对称面
    ax.plot([0, 0], [0, cy * 1.55], "--", color=GREY, lw=1)
    # 夹紧力（向下）
    ax.annotate("", xy=(0, cy + r * 0.15), xytext=(0, cy + r * 1.25),
                arrowprops=dict(arrowstyle="->", color=BAD, lw=2))
    ax.text(r * 0.12, cy + r * 0.95, "夹紧力 F", color=BAD, fontsize=10)
    # 两斜面法向反力
    for s in (-1, 1):
        nx, ny = s * np.cos(half), np.sin(half)
        px, py = -s * r * np.sin(half) * 0 + s * (-r) * np.cos(half) * 0, 0
        # 接触点：圆心沿斜面法向投影
        contact = (0 - s * r * np.cos(half), cy - r * np.sin(half))
        ax.annotate("", xy=(contact[0] + s * r * 0.5 * np.cos(half), contact[1] + r * 0.5 * np.sin(half)),
                    xytext=contact, arrowprops=dict(arrowstyle="->", color=OK, lw=1.8))
        ax.plot([contact[0]], [contact[1]], "o", color=OK, ms=5)
    ax.text(-r * 1.5, cy - r * 0.2, "法向反力 N", color=OK, fontsize=10)
    # 角度标注
    ax.text(r * 0.06, r * 0.28, f"α = {alpha:.0f}°", fontsize=11, color=INK)
    ax.text(0, cy + r * 0.02, f"  D = {D:.0f} mm", fontsize=10, color=BRAND)

    ax.set_xlim(-L * 0.95, L * 0.95)
    ax.set_ylim(-r * 0.3, cy + r * 1.6)
    ax.set_aspect("equal"); ax.axis("off")
    ax.set_title(f"V形块定位（α={alpha:.0f}°，工件 Φ{D:.0f}）\n"
                 f"接触点在两斜面上，夹紧力沿对称面", fontsize=10, color=INK)
    plt.tight_layout()
    _save(fig, out)


def tolerance_zone(p, out):
    """公差带示意图：零线 + 各要素的上下偏差带。"""
    nom = float(p.get("nominal", 30))
    items = p.get("items", [])
    fig, ax = plt.subplots(figsize=(7.2, 4.2))
    ax.axhline(0, color=INK, lw=1.6)
    ax.text(-0.42, 0.0, f"零线\n(基本尺寸 {nom})", fontsize=9, color=INK, va="center", ha="right")

    allv = [v for it in items for v in (float(it["up"]), float(it["low"]))]
    span = max(abs(min(allv)), abs(max(allv))) * 1.5 or 0.05
    for i, it in enumerate(items):
        up, low = float(it["up"]), float(it["low"])
        x = i * 1.0
        c = BRAND if up >= 0 else BAD
        ax.add_patch(Rectangle((x - 0.28, low), 0.56, up - low,
                               fc=c, alpha=.28, ec=c, lw=1.8))
        ax.text(x, up + span * 0.09, f"ES/es = {up:+.3f}", ha="center", fontsize=8.6, color=c)
        ax.text(x, low - span * 0.14, f"EI/ei = {low:+.3f}", ha="center", fontsize=8.6, color=c)
        ax.text(x, -span * 1.28, it["name"], ha="center", fontsize=10, color=INK)

    ax.set_xlim(-0.7, len(items) - 0.3)
    ax.set_ylim(-span * 1.45, span * 1.15)
    ax.set_ylabel("偏差 (mm)", fontsize=9)
    ax.set_xticks([]); ax.spines[["top", "right", "bottom"]].set_visible(False)
    ax.set_title("公差带示意（数值由题目直接给出，图形按比例绘制）", fontsize=10, color=INK)
    plt.tight_layout()
    _save(fig, out)


def _save(fig, out):
    path = out if os.path.isabs(out) else os.path.join(
        os.path.dirname(os.path.dirname(os.path.abspath(__file__))), out)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    fig.savefig(path, dpi=150, bbox_inches="tight", facecolor="white")
    plt.close(fig)
    print(f"已生成：{path}")


TYPES = {"clearance_fit": clearance_fit, "v_block": v_block, "tolerance_zone": tolerance_zone}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--type", required=True, choices=list(TYPES))
    ap.add_argument("--params", required=True, help="JSON 字符串，题目里的数值")
    ap.add_argument("--out", required=True, help="输出路径，如 assets/images/ch3/x.png")
    a = ap.parse_args()
    TYPES[a.type](json.loads(a.params), a.out)


if __name__ == "__main__":
    main()
