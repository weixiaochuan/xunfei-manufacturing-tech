#!/usr/bin/env python3
"""Theme-independent space-utilization checker for Pomegranate native SVG pages.

The checker measures rendered elements in Chromium, but scores occupancy on a
grid instead of using one outer bounding box.  Decorative backgrounds are kept
separate from information-bearing content so that a full-page rectangle, two
distant dots, or a thin guide line cannot make an otherwise empty page pass.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import unittest
from pathlib import Path
from typing import Any

try:  # Materialized production helper name.
    from ppt_native_text_geometry_v1 import open_browser
except ModuleNotFoundError:  # Source-tree test name.
    from ppt_native_text_geometry import open_browser


CANVAS_WIDTH = 1280.0
CANVAS_HEIGHT = 720.0
SAFE = {"x": 48.0, "y": 36.0, "width": 1184.0, "height": 648.0}
GRID_COLUMNS = 32
GRID_ROWS = 18


MEASURE_SCRIPT = r"""
(() => {
  const root = document.documentElement;
  const rootRect = root.getBoundingClientRect();
  const viewBox = root.viewBox && root.viewBox.baseVal;
  const vbWidth = viewBox && viewBox.width ? viewBox.width : 1280;
  const vbHeight = viewBox && viewBox.height ? viewBox.height : 720;
  const scaleX = vbWidth / Math.max(1, rootRect.width);
  const scaleY = vbHeight / Math.max(1, rootRect.height);
  const nodes = [...root.querySelectorAll('text,rect,circle,ellipse,line,polyline,polygon,path,image')];
  const elements = [];
  for (let index = 0; index < nodes.length; index += 1) {
    const element = nodes[index];
    const style = getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden') continue;
    const opacity = Number.parseFloat(style.opacity || '1');
    const fillOpacity = Number.parseFloat(style.fillOpacity || '1');
    const strokeOpacity = Number.parseFloat(style.strokeOpacity || '1');
    if (!(opacity > 0.005)) continue;
    const rect = element.getBoundingClientRect();
    if (!(rect.width > 0 || rect.height > 0)) continue;
    const box = {
      x: (rect.left - rootRect.left) * scaleX,
      y: (rect.top - rootRect.top) * scaleY,
      width: rect.width * scaleX,
      height: rect.height * scaleY,
    };
    const text = element.tagName.toLowerCase() === 'text'
      ? (element.textContent || '').replace(/\s+/g, ' ').trim()
      : '';
    elements.push({
      index,
      tag: element.tagName.toLowerCase(),
      id: element.id || '',
      role: element.getAttribute('data-pome-role') || '',
      regionId: element.getAttribute('data-pome-region-id') || '',
      obstacle: element.getAttribute('data-pome-obstacle') === 'true',
      text,
      box,
      fill: style.fill || '',
      stroke: style.stroke || '',
      opacity: Number.isFinite(opacity) ? opacity : 1,
      fillOpacity: Number.isFinite(fillOpacity) ? fillOpacity : 1,
      strokeOpacity: Number.isFinite(strokeOpacity) ? strokeOpacity : 1,
      strokeWidth: Number.parseFloat(style.strokeWidth || '0') || 0,
    });
  }
  return {width: vbWidth, height: vbHeight, elements};
})()
"""


def _clamp_box(raw: dict[str, Any]) -> dict[str, float] | None:
    x1 = max(SAFE["x"], float(raw.get("x") or 0.0))
    y1 = max(SAFE["y"], float(raw.get("y") or 0.0))
    x2 = min(SAFE["x"] + SAFE["width"], float(raw.get("x") or 0.0) + float(raw.get("width") or 0.0))
    y2 = min(SAFE["y"] + SAFE["height"], float(raw.get("y") or 0.0) + float(raw.get("height") or 0.0))
    if x2 <= x1 or y2 <= y1:
        return None
    return {"x": x1, "y": y1, "width": x2 - x1, "height": y2 - y1}


def _area(box: dict[str, float]) -> float:
    return max(0.0, box["width"]) * max(0.0, box["height"])


def _contains_point(box: dict[str, float], x: float, y: float) -> bool:
    return box["x"] <= x <= box["x"] + box["width"] and box["y"] <= y <= box["y"] + box["height"]


def _new_grid() -> list[list[bool]]:
    return [[False for _ in range(GRID_COLUMNS)] for _ in range(GRID_ROWS)]


def _mark_box(grid: list[list[bool]], box: dict[str, float], padding: float = 0.0, border_only: bool = False) -> None:
    cell_w = SAFE["width"] / GRID_COLUMNS
    cell_h = SAFE["height"] / GRID_ROWS
    x1 = max(SAFE["x"], box["x"] - padding)
    y1 = max(SAFE["y"], box["y"] - padding)
    x2 = min(SAFE["x"] + SAFE["width"], box["x"] + box["width"] + padding)
    y2 = min(SAFE["y"] + SAFE["height"], box["y"] + box["height"] + padding)
    if x2 <= x1 or y2 <= y1:
        return
    col1 = max(0, int((x1 - SAFE["x"]) / cell_w))
    row1 = max(0, int((y1 - SAFE["y"]) / cell_h))
    col2 = min(GRID_COLUMNS - 1, int((x2 - SAFE["x"] - 0.001) / cell_w))
    row2 = min(GRID_ROWS - 1, int((y2 - SAFE["y"] - 0.001) / cell_h))
    for row in range(row1, row2 + 1):
        for col in range(col1, col2 + 1):
            if border_only and row not in (row1, row2) and col not in (col1, col2):
                continue
            grid[row][col] = True


def _grid_ratio(grid: list[list[bool]]) -> float:
    occupied = sum(1 for row in grid for value in row if value)
    return occupied / float(GRID_COLUMNS * GRID_ROWS)


def _merge_grids(*grids: list[list[bool]]) -> list[list[bool]]:
    return [
        [any(grid[row][col] for grid in grids) for col in range(GRID_COLUMNS)]
        for row in range(GRID_ROWS)
    ]


def _largest_empty_rectangle(grid: list[list[bool]]) -> dict[str, float]:
    heights = [0] * GRID_COLUMNS
    best = (0, 0, 0, 0, 0)  # area, left, top, width, height
    for row in range(GRID_ROWS):
        for col in range(GRID_COLUMNS):
            heights[col] = 0 if grid[row][col] else heights[col] + 1
        stack: list[int] = []
        for col in range(GRID_COLUMNS + 1):
            height = heights[col] if col < GRID_COLUMNS else 0
            while stack and heights[stack[-1]] > height:
                index = stack.pop()
                h = heights[index]
                left = stack[-1] + 1 if stack else 0
                width = col - left
                area = h * width
                if area > best[0]:
                    best = (area, left, row - h + 1, width, h)
            stack.append(col)
    area, left, top, width, height = best
    cell_w = SAFE["width"] / GRID_COLUMNS
    cell_h = SAFE["height"] / GRID_ROWS
    return {
        "x": SAFE["x"] + left * cell_w,
        "y": SAFE["y"] + top * cell_h,
        "width": width * cell_w,
        "height": height * cell_h,
        "ratio": area / float(GRID_COLUMNS * GRID_ROWS),
    }


def _zone_stats(grid: list[list[bool]]) -> tuple[list[dict[str, Any]], int, float]:
    zones: list[dict[str, Any]] = []
    total = max(1, sum(1 for row in grid for value in row if value))
    occupied_zones = 0
    dominant = 0
    for zone_row in range(3):
        for zone_col in range(3):
            row1 = round(zone_row * GRID_ROWS / 3)
            row2 = round((zone_row + 1) * GRID_ROWS / 3)
            col1 = round(zone_col * GRID_COLUMNS / 3)
            col2 = round((zone_col + 1) * GRID_COLUMNS / 3)
            count = sum(
                1
                for row in range(row1, row2)
                for col in range(col1, col2)
                if grid[row][col]
            )
            if count >= 3:
                occupied_zones += 1
            dominant = max(dominant, count)
            zones.append({"row": zone_row, "column": zone_col, "occupiedCells": count})
    return zones, occupied_zones, dominant / total


def _band_ratios(grid: list[list[bool]]) -> dict[str, float]:
    def ratio(rows: range, cols: range) -> float:
        total = max(1, len(rows) * len(cols))
        return sum(1 for row in rows for col in cols if grid[row][col]) / total

    bands = {
        "left": ratio(range(GRID_ROWS), range(0, GRID_COLUMNS // 2)),
        "right": ratio(range(GRID_ROWS), range(GRID_COLUMNS // 2, GRID_COLUMNS)),
        "top": ratio(range(0, GRID_ROWS // 2), range(GRID_COLUMNS)),
        "bottom": ratio(range(GRID_ROWS // 2, GRID_ROWS), range(GRID_COLUMNS)),
    }
    for index in range(3):
        row1 = round(index * GRID_ROWS / 3)
        row2 = round((index + 1) * GRID_ROWS / 3)
        bands[f"horizontalThird{index + 1}"] = ratio(range(row1, row2), range(GRID_COLUMNS))
    return bands


def _thresholds(rhythm: str) -> dict[str, float]:
    return {
        "anchor": {"minInformation": 0.085, "minCombined": 0.12, "maxBlank": 0.64, "minZones": 2, "maxDominant": 0.84},
        "breathing": {"minInformation": 0.12, "minCombined": 0.18, "maxBlank": 0.55, "minZones": 3, "maxDominant": 0.74},
        "balanced": {"minInformation": 0.18, "minCombined": 0.24, "maxBlank": 0.43, "minZones": 4, "maxDominant": 0.66},
        "dense": {"minInformation": 0.23, "minCombined": 0.28, "maxBlank": 0.36, "minZones": 5, "maxDominant": 0.60},
    }.get(rhythm, {"minInformation": 0.18, "minCombined": 0.24, "maxBlank": 0.43, "minZones": 4, "maxDominant": 0.66})


def analyze_measurement(measured: dict[str, Any], rhythm: str, expected_units: int, allow_large_whitespace: bool) -> dict[str, Any]:
    information = _new_grid()
    structure = _new_grid()
    texts: list[dict[str, Any]] = []
    graphics: list[dict[str, Any]] = []
    for raw in measured.get("elements", []):
        box = _clamp_box(raw.get("box") or {})
        if box is None:
            continue
        item = dict(raw)
        item["box"] = box
        if item.get("tag") == "text":
            role = str(item.get("role") or "")
            if role == "footer" or (box["y"] >= 674 and box["height"] <= 22):
                continue
            if str(item.get("text") or "").strip():
                texts.append(item)
            continue
        graphics.append(item)

    text_centers = [
        (item["box"]["x"] + item["box"]["width"] / 2, item["box"]["y"] + item["box"]["height"] / 2)
        for item in texts
    ]
    for item in texts:
        _mark_box(information, item["box"], padding=8.0)

    card_count = 0
    substantive_graphic_count = 0
    decorative_graphic_count = 0
    safe_area = SAFE["width"] * SAFE["height"]
    for item in graphics:
        box = item["box"]
        area_ratio = _area(box) / safe_area
        full_background = area_ratio >= 0.78 or (box["width"] >= 1120 and box["height"] >= 580)
        thin_guide = (box["height"] <= 3 and box["width"] >= 800) or (box["width"] <= 3 and box["height"] >= 480)
        if full_background or thin_guide:
            decorative_graphic_count += 1
            continue
        contains_text = any(_contains_point(box, x, y) for x, y in text_centers)
        tag = str(item.get("tag") or "")
        is_card = (
            tag == "rect"
            and contains_text
            and box["width"] >= 90
            and box["height"] >= 42
            and area_ratio <= 0.28
        )
        is_image = tag == "image"
        is_semantic_graphic = bool(item.get("obstacle")) or any(
            token in f"{item.get('id', '')} {item.get('regionId', '')}".lower()
            for token in ("card", "timeline", "process", "chart", "metric", "photo", "image", "quote", "profile")
        )
        if is_card:
            card_count += 1
            substantive_graphic_count += 1
            _mark_box(information, box, padding=2.0)
        elif is_image or is_semantic_graphic:
            substantive_graphic_count += 1
            _mark_box(information, box, padding=4.0, border_only=tag in {"line", "path", "polyline"})
        else:
            decorative_graphic_count += 1
            filled = str(item.get("fill") or "").lower() not in {"", "none", "transparent"}
            fill_visible = filled and float(item.get("fillOpacity") or 0.0) * float(item.get("opacity") or 0.0) >= 0.08
            _mark_box(structure, box, padding=max(2.0, float(item.get("strokeWidth") or 0.0)), border_only=not fill_visible)

    combined = _merge_grids(information, structure)
    information_ratio = _grid_ratio(information)
    structure_ratio = _grid_ratio(structure)
    combined_ratio = _grid_ratio(combined)
    largest_blank = _largest_empty_rectangle(information)
    zones, occupied_zones, dominant_share = _zone_stats(information)
    bands = _band_ratios(information)
    thresholds = _thresholds(rhythm)
    body_text_count = sum(1 for item in texts if str(item.get("role") or "") not in {"title", "subtitle"})
    rendered_units = max(card_count, body_text_count)
    issues: list[dict[str, Any]] = []

    if information_ratio < thresholds["minInformation"]:
        issues.append({"rule": "information_occupancy_too_low", "actual": information_ratio, "required": thresholds["minInformation"]})
    if combined_ratio < thresholds["minCombined"]:
        issues.append({"rule": "visual_structure_too_sparse", "actual": combined_ratio, "required": thresholds["minCombined"]})
    if not allow_large_whitespace and largest_blank["ratio"] > thresholds["maxBlank"]:
        issues.append({"rule": "dead_blank_region_too_large", "actual": largest_blank["ratio"], "maximum": thresholds["maxBlank"], "region": largest_blank})
    if occupied_zones < int(thresholds["minZones"]) or dominant_share > thresholds["maxDominant"]:
        issues.append({"rule": "content_concentrated_in_limited_regions", "occupiedZones": occupied_zones, "requiredZones": int(thresholds["minZones"]), "dominantZoneShare": dominant_share})
    if rhythm in {"balanced", "dense"} and bands["horizontalThird2"] < 0.04:
        issues.append({"rule": "middle_content_band_unused", "actual": bands["horizontalThird2"], "required": 0.04})
    if rhythm in {"balanced", "dense"} and expected_units >= 4 and rendered_units < min(expected_units, 4):
        issues.append({"rule": "expected_content_units_under_rendered", "renderedUnits": rendered_units, "expectedUnits": expected_units})
    if body_text_count == 0 or (body_text_count <= 1 and substantive_graphic_count == 0 and rhythm not in {"anchor", "breathing"}):
        issues.append({"rule": "background_without_substantive_information", "bodyTextCount": body_text_count, "substantiveGraphicCount": substantive_graphic_count})

    return {
        "schemaVersion": 1,
        "passed": not issues,
        "pageRhythm": rhythm,
        "expectedContentUnits": expected_units,
        "informationOccupancyRatio": round(information_ratio, 4),
        "visualStructureRatio": round(structure_ratio, 4),
        "combinedOccupancyRatio": round(combined_ratio, 4),
        "largestEmptyInformationRegion": {key: round(value, 4) for key, value in largest_blank.items()},
        "occupiedZoneCount": occupied_zones,
        "dominantZoneShare": round(dominant_share, 4),
        "zoneStats": zones,
        "bandOccupancy": {key: round(value, 4) for key, value in bands.items()},
        "textBlockCount": len(texts),
        "bodyTextBlockCount": body_text_count,
        "cardCount": card_count,
        "substantiveGraphicCount": substantive_graphic_count,
        "decorativeGraphicCount": decorative_graphic_count,
        "renderedContentUnits": rendered_units,
        "issues": issues,
    }


def measure_svg(path: Path) -> dict[str, Any]:
    session = open_browser()
    try:
        session.command("Page.navigate", {"url": path.resolve().as_uri()})
        session.command("Runtime.evaluate", {"expression": "new Promise(resolve => document.readyState === 'complete' ? resolve(true) : addEventListener('load', () => resolve(true), {once:true}))", "awaitPromise": True})
        evaluated = session.command("Runtime.evaluate", {"expression": MEASURE_SCRIPT, "returnByValue": True})
        result = evaluated.get("result", {})
        if "value" not in result:
            raise RuntimeError(f"Chromium 无法测量 SVG 空间占用: {result}")
        return result["value"]
    finally:
        session.close()


def run(svg: Path, rhythm: str, expected_units: int, allow_large_whitespace: bool) -> dict[str, Any]:
    if not svg.is_file():
        raise RuntimeError(f"SVG 文件不存在: {svg}")
    measured = measure_svg(svg)
    report = analyze_measurement(measured, rhythm, expected_units, allow_large_whitespace)
    report["svgPath"] = str(svg.resolve())
    return report


class SpaceUtilizationTests(unittest.TestCase):
    @staticmethod
    def measurement(elements: list[dict[str, Any]]) -> dict[str, Any]:
        defaults = {"id": "", "role": "", "regionId": "", "obstacle": False, "text": "", "fill": "none", "stroke": "#333", "opacity": 1, "fillOpacity": 1, "strokeOpacity": 1, "strokeWidth": 1}
        return {"elements": [{**defaults, **item} for item in elements]}

    def test_breathing_page_allows_purposeful_whitespace(self) -> None:
        report = analyze_measurement(self.measurement([
            {"tag": "text", "role": "title", "text": "核心结论", "box": {"x": 210, "y": 180, "width": 500, "height": 70}},
            {"tag": "text", "role": "body", "text": "以一个明确观点建立视觉中心", "box": {"x": 210, "y": 285, "width": 620, "height": 70}},
            {"tag": "rect", "id": "quote-panel", "fill": "#eee", "box": {"x": 170, "y": 140, "width": 850, "height": 300}},
        ]), "breathing", 1, True)
        self.assertTrue(report["passed"], report)

    def test_anchor_cover_does_not_require_four_body_blocks(self) -> None:
        report = analyze_measurement(self.measurement([
            {"tag": "text", "role": "title", "text": "封面标题", "box": {"x": 180, "y": 150, "width": 650, "height": 80}},
            {"tag": "text", "role": "subtitle", "text": "明确副标题", "box": {"x": 180, "y": 260, "width": 500, "height": 45}},
            {"tag": "text", "role": "body", "text": "核心主张", "box": {"x": 180, "y": 365, "width": 650, "height": 70}},
            {"tag": "rect", "id": "hero-panel", "fill": "#eee", "box": {"x": 140, "y": 110, "width": 900, "height": 410}},
        ]), "anchor", 4, True)
        self.assertTrue(report["passed"], report)

    def test_balanced_page_detects_top_left_dead_blank(self) -> None:
        report = analyze_measurement(self.measurement([
            {"tag": "text", "role": "title", "text": "标题", "box": {"x": 70, "y": 55, "width": 280, "height": 48}},
            {"tag": "text", "role": "body", "text": "正文一", "box": {"x": 80, "y": 145, "width": 220, "height": 30}},
            {"tag": "text", "role": "body", "text": "正文二", "box": {"x": 80, "y": 195, "width": 220, "height": 30}},
        ]), "balanced", 4, False)
        self.assertFalse(report["passed"])
        self.assertTrue(any(issue["rule"] == "dead_blank_region_too_large" for issue in report["issues"]))

    def test_two_distant_small_elements_do_not_fake_occupancy(self) -> None:
        report = analyze_measurement(self.measurement([
            {"tag": "text", "role": "body", "text": "A", "box": {"x": 55, "y": 45, "width": 20, "height": 20}},
            {"tag": "text", "role": "body", "text": "B", "box": {"x": 1200, "y": 650, "width": 20, "height": 20}},
        ]), "balanced", 2, False)
        self.assertFalse(report["passed"])
        self.assertLess(report["informationOccupancyRatio"], 0.05)

    def test_full_background_does_not_count_as_information(self) -> None:
        report = analyze_measurement(self.measurement([
            {"tag": "rect", "fill": "#fafafa", "box": {"x": 0, "y": 0, "width": 1280, "height": 720}},
            {"tag": "text", "role": "title", "text": "标题", "box": {"x": 70, "y": 55, "width": 280, "height": 48}},
        ]), "balanced", 4, False)
        self.assertFalse(report["passed"])
        self.assertEqual(report["visualStructureRatio"], 0.0)

    def test_content_rich_page_cannot_render_only_two_units(self) -> None:
        report = analyze_measurement(self.measurement([
            {"tag": "text", "role": "title", "text": "六项事实", "box": {"x": 70, "y": 55, "width": 500, "height": 48}},
            {"tag": "rect", "id": "card-left", "fill": "#eee", "box": {"x": 70, "y": 135, "width": 500, "height": 460}},
            {"tag": "text", "role": "body", "text": "只渲染事实一", "box": {"x": 100, "y": 180, "width": 420, "height": 80}},
            {"tag": "rect", "id": "card-right", "fill": "#eee", "box": {"x": 650, "y": 135, "width": 500, "height": 460}},
            {"tag": "text", "role": "body", "text": "只渲染事实二", "box": {"x": 680, "y": 180, "width": 420, "height": 80}},
        ]), "dense", 6, False)
        self.assertFalse(report["passed"])
        self.assertTrue(any(issue["rule"] == "expected_content_units_under_rendered" for issue in report["issues"]))

    def test_balanced_page_rejects_an_entire_unused_middle_band(self) -> None:
        report = analyze_measurement(self.measurement([
            {"tag": "text", "role": "title", "text": "标题", "box": {"x": 80, "y": 55, "width": 500, "height": 48}},
            {"tag": "text", "role": "body", "text": "上方信息", "box": {"x": 80, "y": 130, "width": 500, "height": 50}},
            {"tag": "text", "role": "body", "text": "底部事实一", "box": {"x": 80, "y": 540, "width": 320, "height": 60}},
            {"tag": "text", "role": "body", "text": "底部事实二", "box": {"x": 470, "y": 540, "width": 320, "height": 60}},
            {"tag": "text", "role": "body", "text": "底部事实三", "box": {"x": 860, "y": 540, "width": 300, "height": 60}},
        ]), "balanced", 4, False)
        self.assertFalse(report["passed"])
        self.assertTrue(any(issue["rule"] == "middle_content_band_unused" for issue in report["issues"]))

    def test_same_measurement_uses_same_rules_regardless_of_theme(self) -> None:
        measured = self.measurement([
            {"tag": "text", "role": "body", "text": "通用内容", "box": {"x": 100, "y": 120, "width": 500, "height": 80}},
        ])
        first = analyze_measurement(measured, "balanced", 4, False)
        second = analyze_measurement(measured, "balanced", 4, False)
        self.assertEqual(first, second)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--svg", type=Path)
    parser.add_argument("--page-rhythm", choices=("anchor", "breathing", "balanced", "dense"), default="balanced")
    parser.add_argument("--expected-content-units", type=int, default=3)
    parser.add_argument("--allow-large-whitespace", action="store_true")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(SpaceUtilizationTests))
        return 0 if result.wasSuccessful() else 1
    if args.svg is None:
        parser.error("--svg is required unless --self-test is used")
    try:
        report = run(args.svg, args.page_rhythm, max(0, args.expected_content_units), args.allow_large_whitespace)
        serialized = json.dumps(report, ensure_ascii=False)
        if args.report is not None:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            temporary = args.report.with_name(f".{args.report.name}.{os.getpid()}.tmp")
            temporary.write_text(serialized + "\n", encoding="utf-8")
            os.replace(temporary, args.report)
        print(serialized)
        return 0 if report["passed"] else 2
    except Exception as error:
        print(json.dumps({"schemaVersion": 1, "passed": False, "checkerError": str(error)}, ensure_ascii=False))
        return 3


if __name__ == "__main__":
    sys.exit(main())
