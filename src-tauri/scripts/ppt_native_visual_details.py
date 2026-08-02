#!/usr/bin/env python3
"""Theme-independent visual-detail QA for Pomegranate native SVG slides.

The text geometry checker owns text layout.  This checker deliberately owns
the relationships around that text: cards, nodes, connectors, safe-area
bounds, and small alignment drift.  Explicit ``data-pome-*`` metadata is the
authoritative contract.  Conservative inference is used for old SVGs only;
inferred findings are warnings unless content is outside the canvas.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any

try:
    from ppt_native_text_geometry_v1 import measure_svg, open_browser
except ImportError:  # Direct source-tree execution.
    from ppt_native_text_geometry import measure_svg, open_browser


CANVAS = {"x": 0.0, "y": 0.0, "width": 1280.0, "height": 720.0}
SVG_NS = "http://www.w3.org/2000/svg"
ET.register_namespace("", SVG_NS)


VISUAL_MEASURE_SCRIPT = r"""
(async () => {
  await document.fonts.ready;
  const root = document.documentElement;
  const rootInverse = root.getScreenCTM().inverse();
  function rootPoint(element, x, y) {
    const matrix = rootInverse.multiply(element.getScreenCTM());
    const point = new DOMPoint(x, y).matrixTransform(matrix);
    return {x: point.x, y: point.y};
  }
  function rootBox(element) {
    const box = element.getBBox();
    const points = [
      rootPoint(element, box.x, box.y),
      rootPoint(element, box.x + box.width, box.y),
      rootPoint(element, box.x, box.y + box.height),
      rootPoint(element, box.x + box.width, box.y + box.height),
    ];
    const xs = points.map((point) => point.x);
    const ys = points.map((point) => point.y);
    const x = Math.min(...xs);
    const y = Math.min(...ys);
    return {x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y};
  }
  function visible(element) {
    const style = getComputedStyle(element);
    return style.display !== 'none' && style.visibility !== 'hidden' &&
      Number(style.opacity) !== 0 && Number(style.fillOpacity) !== 0 ||
      (style.display !== 'none' && style.visibility !== 'hidden' &&
       Number(style.opacity) !== 0 && Number(style.strokeOpacity) !== 0);
  }
  const selector = 'g,rect,circle,ellipse,line,polyline,polygon,path,text';
  const elements = [...root.querySelectorAll(selector)].filter((element) => {
    if (element.closest('defs')) return false;
    try { return visible(element) && rootBox(element).width + rootBox(element).height > 0; }
    catch (_) { return false; }
  }).map((element, index) => {
    const tag = element.localName;
    const style = getComputedStyle(element);
    const structuralAncestor = tag === 'text' ? element.parentElement?.closest(
      '[data-pome-visual-role="node"],[data-pome-visual-role="icon"],'
      + '[data-pome-visual-role="card"],[data-pome-visual-role="section"]'
    ) : null;
    const result = {
      elementId: element.id || `visual-${index + 1}`,
      hasExplicitId: Boolean(element.id),
      domIndex: index,
      tag,
      bbox: rootBox(element),
      role: (element.getAttribute('data-pome-visual-role') || '').toLowerCase(),
      owner: element.getAttribute('data-pome-owner'),
      structuralAncestorId: structuralAncestor?.id || null,
      uniformSize: element.getAttribute('data-pome-uniform-size') === 'true',
      alignGroup: element.getAttribute('data-pome-align-group'),
      alignAxis: (element.getAttribute('data-pome-align-axis') || '').toLowerCase(),
      connectorFrom: element.getAttribute('data-pome-from'),
      connectorTo: element.getAttribute('data-pome-to'),
      markerStart: element.getAttribute('marker-start'),
      markerEnd: element.getAttribute('marker-end'),
      allowOverlap: element.getAttribute('data-pome-allow-overlap') === 'true',
      decorative: element.getAttribute('data-pome-decorative') === 'true' ||
        (element.getAttribute('data-pome-visual-role') || '').toLowerCase() === 'decoration',
      fill: style.fill,
      fillOpacity: Number(style.fillOpacity),
      stroke: style.stroke,
      strokeOpacity: Number(style.strokeOpacity),
      strokeWidth: Number.parseFloat(style.strokeWidth) || 0,
      transform: element.getAttribute('transform'),
      structuralDescendantCount: tag === 'g' ? element.querySelectorAll(
        '[data-pome-visual-role="card"],[data-pome-visual-role="section"],'
        + '[data-pome-visual-role="node"],[data-pome-visual-role="icon"],'
        + '[data-pome-visual-role="connector"]'
      ).length : 0,
      graphicalDescendantCount: tag === 'g' ? element.querySelectorAll(
        'rect,circle,ellipse,line,polyline,polygon,path,text'
      ).length : 0,
      text: tag === 'text' ? (element.textContent || '').replace(/\s+/g, ' ').trim() : '',
    };
    if (tag === 'g' && (result.role === 'node' || result.role === 'card')) {
      const anchorCandidates = [...element.children].filter((child) =>
        result.role === 'node'
          ? child.localName === 'circle' || child.localName === 'ellipse'
          : child.localName === 'rect'
      ).map((child) => ({element: child, box: rootBox(child)}));
      if (anchorCandidates.length > 0) {
        anchorCandidates.sort((left, right) =>
          right.box.width * right.box.height - left.box.width * left.box.height
        );
        result.anchorBox = anchorCandidates[0].box;
        result.anchorShape = anchorCandidates[0].element.localName;
      }
    }
    if (tag === 'line') {
      result.start = rootPoint(element, Number(element.getAttribute('x1') || 0), Number(element.getAttribute('y1') || 0));
      result.end = rootPoint(element, Number(element.getAttribute('x2') || 0), Number(element.getAttribute('y2') || 0));
      result.routePoints = [result.start, result.end];
    } else if (tag === 'polyline') {
      const localPoints = [...element.points].map((point) => rootPoint(element, point.x, point.y));
      if (localPoints.length >= 2) {
        result.start = localPoints[0];
        result.end = localPoints[localPoints.length - 1];
        result.routePoints = localPoints;
      }
    } else if (tag === 'path') {
      try {
        const length = element.getTotalLength();
        if (length > 0) {
          result.routePoints = Array.from({length: 17}, (_, pointIndex) => {
            const point = element.getPointAtLength(length * pointIndex / 16);
            return rootPoint(element, point.x, point.y);
          });
          result.start = result.routePoints[0];
          result.end = result.routePoints[result.routePoints.length - 1];
        }
      } catch (_) {}
    }
    return result;
  });
  return elements;
})()
"""


def _right(box: dict[str, float]) -> float:
    return box["x"] + box["width"]


def _bottom(box: dict[str, float]) -> float:
    return box["y"] + box["height"]


def _area(box: dict[str, float]) -> float:
    return max(0.0, box["width"]) * max(0.0, box["height"])


def _intersection(left: dict[str, float], right: dict[str, float]) -> dict[str, float] | None:
    x1, y1 = max(left["x"], right["x"]), max(left["y"], right["y"])
    x2, y2 = min(_right(left), _right(right)), min(_bottom(left), _bottom(right))
    if x2 <= x1 or y2 <= y1:
        return None
    return {"x": x1, "y": y1, "width": x2 - x1, "height": y2 - y1}


def _contains(outer: dict[str, float], inner: dict[str, float], tolerance: float = 1.0) -> bool:
    return (
        inner["x"] >= outer["x"] - tolerance
        and inner["y"] >= outer["y"] - tolerance
        and _right(inner) <= _right(outer) + tolerance
        and _bottom(inner) <= _bottom(outer) + tolerance
    )


def _overflow(box: dict[str, float], allowed: dict[str, float]) -> dict[str, float]:
    return {
        "left": max(0.0, allowed["x"] - box["x"]),
        "top": max(0.0, allowed["y"] - box["y"]),
        "right": max(0.0, _right(box) - _right(allowed)),
        "bottom": max(0.0, _bottom(box) - _bottom(allowed)),
    }


def _has_overflow(values: dict[str, float], tolerance: float = 0.75) -> bool:
    return any(value > tolerance for value in values.values())


def _point_distance(left: dict[str, float], right: dict[str, float]) -> float:
    return math.hypot(left["x"] - right["x"], left["y"] - right["y"])


def _nearest_box_anchor(box: dict[str, float], toward: dict[str, float]) -> dict[str, float]:
    center = {"x": box["x"] + box["width"] / 2, "y": box["y"] + box["height"] / 2}
    dx, dy = toward["x"] - center["x"], toward["y"] - center["y"]
    if abs(dx) < 1e-9 and abs(dy) < 1e-9:
        return center
    scale_x = box["width"] / 2 / abs(dx) if abs(dx) > 1e-9 else float("inf")
    scale_y = box["height"] / 2 / abs(dy) if abs(dy) > 1e-9 else float("inf")
    scale = min(scale_x, scale_y)
    return {"x": center["x"] + dx * scale, "y": center["y"] + dy * scale}


def _nearest_anchor(element: dict[str, Any], toward: dict[str, float]) -> dict[str, float]:
    route_points = element.get("routePoints") or []
    if len(route_points) >= 2:
        candidates: list[dict[str, float]] = []
        for start, end in zip(route_points, route_points[1:]):
            dx, dy = end["x"] - start["x"], end["y"] - start["y"]
            length_squared = dx * dx + dy * dy
            if length_squared <= 1e-9:
                candidates.append(dict(start))
                continue
            projection = (
                (toward["x"] - start["x"]) * dx
                + (toward["y"] - start["y"]) * dy
            ) / length_squared
            projection = min(1.0, max(0.0, projection))
            candidates.append(
                {
                    "x": start["x"] + projection * dx,
                    "y": start["y"] + projection * dy,
                }
            )
        return min(candidates, key=lambda point: _point_distance(point, toward))
    box = element.get("anchorBox") or element["bbox"]
    if not element.get("anchorBox") and element.get("tag") not in {"circle", "ellipse"}:
        return _nearest_box_anchor(box, toward)
    center = {"x": box["x"] + box["width"] / 2, "y": box["y"] + box["height"] / 2}
    dx, dy = toward["x"] - center["x"], toward["y"] - center["y"]
    rx, ry = box["width"] / 2, box["height"] / 2
    denominator = math.sqrt((dx * dx) / max(rx * rx, 1e-9) + (dy * dy) / max(ry * ry, 1e-9))
    if denominator < 1e-9:
        return center
    return {"x": center["x"] + dx / denominator, "y": center["y"] + dy / denominator}


def _distance_to_anchor_boundary(
    element: dict[str, Any], point: dict[str, float]
) -> float:
    """Return the shortest distance from a point to the declared shape boundary.

    A connector may legitimately attach anywhere on a card edge.  The radial
    projection toward the opposite endpoint remains useful when repair is
    needed, but it must not reject an endpoint that already touches another
    valid point on the same boundary (including a rounded-card corner).
    """
    anchor_shape = element.get("anchorShape") or element.get("tag")
    if anchor_shape in {"circle", "ellipse"}:
        return _point_distance(point, _nearest_anchor(element, point))
    box = element.get("anchorBox") or element["bbox"]
    left, top = float(box["x"]), float(box["y"])
    right, bottom = _right(box), _bottom(box)
    x, y = float(point["x"]), float(point["y"])
    if left <= x <= right and top <= y <= bottom:
        return min(x - left, right - x, y - top, bottom - y)
    nearest_x = min(right, max(left, x))
    nearest_y = min(bottom, max(top, y))
    return math.hypot(x - nearest_x, y - nearest_y)


def _segment_intersects_box(start: dict[str, float], end: dict[str, float], box: dict[str, float]) -> bool:
    # Liang-Barsky clipping; inset avoids treating a line touching the edge as collision.
    inset = 1.5
    left, right = box["x"] + inset, _right(box) - inset
    top, bottom = box["y"] + inset, _bottom(box) - inset
    if right <= left or bottom <= top:
        return False
    dx, dy = end["x"] - start["x"], end["y"] - start["y"]
    p = (-dx, dx, -dy, dy)
    q = (start["x"] - left, right - start["x"], start["y"] - top, bottom - start["y"])
    low, high = 0.0, 1.0
    for pi, qi in zip(p, q):
        if abs(pi) < 1e-9:
            if qi < 0:
                return False
            continue
        ratio = qi / pi
        if pi < 0:
            low = max(low, ratio)
        else:
            high = min(high, ratio)
        if low > high:
            return False
    return True


def _point_to_segment_distance(
    point: dict[str, float], start: dict[str, float], end: dict[str, float]
) -> float:
    dx, dy = end["x"] - start["x"], end["y"] - start["y"]
    length_squared = dx * dx + dy * dy
    if length_squared <= 1e-9:
        return _point_distance(point, start)
    projection = (
        (point["x"] - start["x"]) * dx + (point["y"] - start["y"]) * dy
    ) / length_squared
    projection = min(1.0, max(0.0, projection))
    nearest = {
        "x": start["x"] + projection * dx,
        "y": start["y"] + projection * dy,
    }
    return _point_distance(point, nearest)


def _issue(rule: str, element: dict[str, Any], message: str, *, severity: str = "hard", **extra: Any) -> dict[str, Any]:
    value: dict[str, Any] = {
        "severity": severity,
        "rule": rule,
        "elementId": element.get("elementId"),
        "domIndex": element.get("domIndex"),
        "role": element.get("role"),
        "actualBounds": element.get("bbox"),
        "message": message,
    }
    value.update(extra)
    return value


def _infer_cards(elements: list[dict[str, Any]], texts: list[dict[str, Any]]) -> list[dict[str, Any]]:
    candidates: list[dict[str, Any]] = []
    for element in elements:
        explicit = element.get("role") == "card"
        box = (element.get("anchorBox") if explicit else None) or element.get("bbox") or {}
        inferred = (
            element.get("tag") == "rect"
            and 100 <= box.get("width", 0) <= 900
            and 44 <= box.get("height", 0) <= 560
            and _area(box) < 1280 * 720 * 0.72
            and not element.get("decorative")
            and any(_contains(box, text.get("bbox") or {}) for text in texts)
        )
        if explicit or inferred:
            item = dict(element)
            # Composite card groups often contain their leader and node in addition
            # to the visible panel.  Collision and connector checks must use the
            # panel surface, not the union bbox of every descendant.
            item["bbox"] = box
            item["inferred"] = not explicit
            candidates.append(item)
    # Remove nested inferred surfaces; the smallest containing surface owns text padding.
    # A semantic <g role="card"> usually wraps the visible background <rect>.
    # Browser measurement reports both boxes, so keep the semantic group and discard
    # an inferred surface that covers substantially the same footprint. Otherwise a
    # single card is counted twice and every real neighbour collision is duplicated.
    result: list[dict[str, Any]] = []
    for candidate in candidates:
        if candidate.get("inferred"):
            candidate_area = _area(candidate["bbox"])
            if any(
                other is not candidate
                and not other.get("inferred")
                and _contains(other["bbox"], candidate["bbox"], 1)
                and candidate_area / max(_area(other["bbox"]), 1.0) >= 0.8
                for other in candidates
            ):
                continue
            if any(
                other is not candidate
                and _area(other["bbox"]) < candidate_area
                and _contains(candidate["bbox"], other["bbox"], 0)
                for other in candidates
            ):
                continue
        result.append(candidate)
    return result


def classify_measurements(measured: dict[str, Any], require_contract: bool = False) -> dict[str, Any]:
    elements = measured.get("elements", [])
    texts = measured.get("texts", [])
    hard: list[dict[str, Any]] = []
    warnings: list[dict[str, Any]] = []
    by_id = {item.get("elementId"): item for item in elements if item.get("hasExplicitId")}
    for text in texts:
        region_id = text.get("regionId")
        region = text.get("region")
        if not region_id or not region or region_id in by_id:
            continue
        by_id[region_id] = {
            **text,
            "elementId": region_id,
            "bbox": region,
            "anchorBox": region,
            "anchorShape": "rect",
            "targetKind": "text-region",
        }
    cards = _infer_cards(elements, texts)

    def composite_container(item: dict[str, Any]) -> bool:
        return item.get("tag") == "g" and (
            item.get("structuralDescendantCount", 0) > 0
            or item.get("graphicalDescendantCount", 0) > 0
        )

    def short_decorative_leader(item: dict[str, Any]) -> bool:
        if item.get("tag") not in {"line", "polyline", "path"}:
            return False
        start, end = item.get("start"), item.get("end")
        if not start or not end or item.get("markerStart") or item.get("markerEnd"):
            return False
        if _point_distance(start, end) > 32:
            return False
        target_ids = [item.get("connectorFrom"), item.get("connectorTo")]
        return any(target_id and target_id not in by_id for target_id in target_ids)

    decorative_leaders = {
        item.get("elementId") for item in elements if short_decorative_leader(item)
    }

    def shared_structural_axis(item: dict[str, Any]) -> bool:
        tag = item.get("tag")
        if tag == "g":
            box = item.get("bbox") or {}
            width = float(box.get("width", 0))
            height = float(box.get("height", 0))
            if width >= 480 and height <= 40:
                center_y = float(box["y"]) + height / 2
                route = [
                    {"x": float(box["x"]), "y": center_y},
                    {"x": _right(box), "y": center_y},
                ]
            elif height >= 320 and width <= 40:
                center_x = float(box["x"]) + width / 2
                route = [
                    {"x": center_x, "y": float(box["y"])},
                    {"x": center_x, "y": _bottom(box)},
                ]
            else:
                return False
        elif tag in {"line", "polyline", "path"}:
            route = item.get("routePoints") or []
        else:
            return False
        if len(route) < 2 or sum(
            _point_distance(start, end) for start, end in zip(route, route[1:])
        ) < 320:
            return False
        supported_nodes = 0
        for node in elements:
            is_semantic_node = node.get("role") == "node"
            is_explicit_circle_marker = (
                node.get("hasExplicitId")
                and node.get("tag") in {"circle", "ellipse"}
                and 6 <= float((node.get("bbox") or {}).get("width", 0)) <= 100
                and 6 <= float((node.get("bbox") or {}).get("height", 0)) <= 100
            )
            if (not is_semantic_node and not is_explicit_circle_marker) or not node.get("hasExplicitId"):
                continue
            box = node.get("anchorBox") or node.get("bbox") or {}
            if not box:
                continue
            center = {
                "x": float(box["x"]) + float(box["width"]) / 2,
                "y": float(box["y"]) + float(box["height"]) / 2,
            }
            if min(
                _point_to_segment_distance(center, start, end)
                for start, end in zip(route, route[1:])
            ) <= 14:
                supported_nodes += 1
        return supported_nodes >= 3

    structural_axes = {
        item.get("elementId") for item in elements if shared_structural_axis(item)
    }

    structural_roles = {"card", "section", "node", "icon", "label", "connector"}
    explicit_structural = [item for item in elements if item.get("role") in structural_roles]
    likely_lines = []
    for item in elements:
        if item.get("tag") not in {"line", "polyline", "path"} or not item.get("start") or not item.get("end"):
            continue
        length = _point_distance(item["start"], item["end"])
        horizontal_separator = abs(item["start"]["y"] - item["end"]["y"]) < 1 and length > 900
        if 20 <= length <= 900 and not horizontal_separator:
            likely_lines.append(item)
    likely_nodes = [item for item in elements if item.get("tag") in {"circle", "ellipse"} and 6 <= item["bbox"]["width"] <= 100 and 6 <= item["bbox"]["height"] <= 100]
    if require_contract and not explicit_structural and (len(cards) >= 2 or (len(likely_nodes) >= 2 and likely_lines)):
        hard.append(_issue("missing_visual_detail_metadata", {"elementId": None, "domIndex": None, "role": None, "bbox": CANVAS}, "页面包含多卡片或节点连线结构，但没有声明 data-pome-visual-role/id/anchor 合同", inferredCards=len(cards), inferredNodes=len(likely_nodes), inferredLines=len(likely_lines)))
    for element in elements:
        if element.get("role") not in structural_roles or element.get("decorative"):
            continue
        if element.get("elementId") in decorative_leaders:
            continue
        if not element.get("hasExplicitId"):
            hard.append(_issue("missing_visual_element_id", element, "结构元素缺少稳定 id，无法执行锚点与对齐检查"))
        if (
            element.get("role") == "connector"
            and element.get("elementId") not in structural_axes
            and (not element.get("connectorFrom") or not element.get("connectorTo"))
        ):
            hard.append(_issue("connector_contract_incomplete", element, "连接线必须同时声明 data-pome-from 与 data-pome-to"))
        overflow = _overflow(element["bbox"], CANVAS)
        if _has_overflow(overflow):
            hard.append(_issue("element_outside_safe_area", element, "结构元素超出 1280×720 画布", overflow=overflow))

    for index, left in enumerate(cards):
        for right in cards[index + 1 :]:
            if left.get("allowOverlap") or right.get("allowOverlap"):
                continue
            if _contains(left["bbox"], right["bbox"], 1) or _contains(right["bbox"], left["bbox"], 1):
                continue
            overlap = _intersection(left["bbox"], right["bbox"])
            if overlap is None or overlap["width"] < 3 or overlap["height"] < 3 or _area(overlap) < 36:
                continue
            severity = "warning" if left.get("inferred") and right.get("inferred") else "hard"
            target = hard if severity == "hard" else warnings
            target.append(_issue("card_card_overlap", left, "卡片或分区框发生可见互压", severity=severity, collision={"elementId": right.get("elementId"), "bounds": right.get("bbox"), "intersection": overlap}))

    # Only explicit semantic shapes participate here. Nested icons/nodes must
    # declare their owner (or intentional overlap) so decoration is not
    # mistaken for a collision.
    semantic_shapes = [
        item
        for item in elements
        if item.get("role") in {"node", "icon"}
        and not item.get("decorative")
        and not composite_container(item)
    ]
    for index, left in enumerate(semantic_shapes):
        for right in semantic_shapes[index + 1 :]:
            if left.get("allowOverlap") or right.get("allowOverlap"):
                continue
            if left.get("owner") == right.get("elementId") or right.get("owner") == left.get("elementId"):
                continue
            overlap = _intersection(left["bbox"], right["bbox"])
            if overlap is None or _area(overlap) < 16:
                continue
            hard.append(_issue("shape_shape_overlap", left, "节点或图标发生非声明的可见互压", collision={"elementId": right.get("elementId"), "bounds": right.get("bbox"), "intersection": overlap}))

    for text in texts:
        text_box = text.get("bbox") or {}
        owner_id = text.get("owner")
        owners = [card for card in cards if card.get("elementId") == owner_id] if owner_id else [card for card in cards if _contains(card["bbox"], text_box)]
        if not owners:
            continue
        owner = min(owners, key=lambda item: _area(item["bbox"]))
        if text.get("regionId") and text.get("region"):
            overlap = _intersection(text_box, owner["bbox"])
            overlap_ratio = (_area(overlap) if overlap else 0.0) / max(_area(text_box), 1.0)
            if _contains(owner["bbox"], text_box, 1) or overlap_ratio >= 0.5:
                continue
        padding = 8.0
        inner = {"x": owner["bbox"]["x"] + padding, "y": owner["bbox"]["y"] + padding, "width": max(0.0, owner["bbox"]["width"] - padding * 2), "height": max(0.0, owner["bbox"]["height"] - padding * 2)}
        overflow = _overflow(text_box, inner)
        if not _has_overflow(overflow):
            continue
        callout_leader = False
        if text.get("role") == "label":
            expanded_text = {
                "x": float(text_box["x"]) - 8,
                "y": float(text_box["y"]) - 8,
                "width": float(text_box["width"]) + 16,
                "height": float(text_box["height"]) + 16,
            }
            for line in elements:
                if line.get("tag") not in {"line", "polyline"}:
                    continue
                start, end = line.get("start"), line.get("end")
                if (
                    not start
                    or not end
                    or _point_distance(start, end) > 80
                ):
                    continue
                start_near_label = (
                    expanded_text["x"] <= float(start["x"]) <= _right(expanded_text)
                    and expanded_text["y"] <= float(start["y"]) <= _bottom(expanded_text)
                )
                end_near_label = (
                    expanded_text["x"] <= float(end["x"]) <= _right(expanded_text)
                    and expanded_text["y"] <= float(end["y"]) <= _bottom(expanded_text)
                )
                start_in_owner = (
                    float(owner["bbox"]["x"]) <= float(start["x"]) <= _right(owner["bbox"])
                    and float(owner["bbox"]["y"]) <= float(start["y"]) <= _bottom(owner["bbox"])
                )
                end_in_owner = (
                    float(owner["bbox"]["x"]) <= float(end["x"]) <= _right(owner["bbox"])
                    and float(owner["bbox"]["y"]) <= float(end["y"]) <= _bottom(owner["bbox"])
                )
                if (start_near_label and end_in_owner) or (
                    end_near_label and start_in_owner
                ):
                    callout_leader = True
                    break
        if callout_leader:
            continue
        explicit = bool(owner_id) or not owner.get("inferred")
        severity = "hard" if explicit else "warning"
        target = hard if severity == "hard" else warnings
        rule = "label_outside_region" if text.get("role") == "label" else "text_shape_overlap"
        target.append(_issue(rule, text, "文字未保留所属区域的安全内边距", severity=severity, ownerId=owner.get("elementId"), allowedBounds=inner, overflow=overflow))

    icons_and_nodes = [
        item
        for item in elements
        if item.get("role") in {"icon", "node"} and not composite_container(item)
    ]
    for text in texts:
        if text.get("allowOverlap"):
            continue
        for shape in icons_and_nodes:
            if text.get("owner") == shape.get("elementId"):
                continue
            # A node may be authored as a sibling shape plus a centered label
            # instead of a wrapping <g>.  Complete geometric containment proves
            # the intended node-label relationship even when data-pome-owner
            # points at the enclosing section.  Partial intersections and body
            # text remain hard collisions.
            if (
                text.get("role") == "label"
                and _contains(
                    shape.get("anchorBox") or shape.get("bbox") or {},
                    text.get("bbox") or {},
                    1.5,
                )
            ):
                continue
            overlap = _intersection(text.get("bbox") or {}, shape.get("bbox") or {})
            if overlap is None or _area(overlap) < 4:
                continue
            hard.append(
                _issue(
                    "icon_label_collision",
                    text,
                    "标签或正文压到图标/节点",
                    ownerId=text.get("owner"),
                    regionId=text.get("regionId"),
                    collision={"elementId": shape.get("elementId"), "bounds": shape.get("bbox"), "intersection": overlap},
                )
            )

    connector_roles = {"connector", "timeline", "relationship-line"}
    connectors = [
        item
        for item in elements
        if item.get("tag") in {"line", "polyline", "path"}
        and (item.get("role") in connector_roles or item.get("connectorFrom") or item.get("connectorTo"))
        and item.get("elementId") not in decorative_leaders
    ]
    for connector in connectors:
        start, end = connector.get("start"), connector.get("end")
        if not start or not end:
            continue
        endpoint_pairs = (
            ()
            if connector.get("elementId") in structural_axes
            else (
                ("start", connector.get("connectorFrom"), start, end),
                ("end", connector.get("connectorTo"), end, start),
            )
        )
        for endpoint_name, target_id, endpoint, toward in endpoint_pairs:
            if not target_id:
                continue
            target = by_id.get(target_id)
            if target is None:
                hard.append(_issue("connector_endpoint_not_on_node", connector, f"连接线声明了不存在的锚点 {target_id}", endpoint=endpoint_name, targetId=target_id))
                continue
            if target.get("targetKind") == "text-region":
                target_box = target["bbox"]
                if (
                    float(target_box["x"]) - 1.5
                    <= float(endpoint["x"])
                    <= _right(target_box) + 1.5
                    and float(target_box["y"]) - 1.5
                    <= float(endpoint["y"])
                    <= _bottom(target_box) + 1.5
                ):
                    continue
            if target.get("decorative") or target.get("role") == "decoration":
                # A decorative silhouette/glow/group has no stable semantic anchor. Keep
                # checking the line against text, but do not force its ornamental endpoint
                # onto the aggregate browser bounding box of that composite decoration.
                continue
            expected = _nearest_anchor(target, toward)
            distance = _distance_to_anchor_boundary(target, endpoint)
            if distance > 1.5:
                other_targets = [
                    item
                    for item in elements
                    if item.get("elementId") != target_id
                    and item.get("role") in {"node", "card", "section"}
                    and item.get("hasExplicitId")
                ]
                nearest_other = min(
                    (
                        (_distance_to_anchor_boundary(item, endpoint), item)
                        for item in other_targets
                    ),
                    default=(float("inf"), None),
                    key=lambda pair: pair[0],
                )
                if nearest_other[1] is not None and nearest_other[0] + 1.5 < distance:
                    hard.append(_issue("visual_anchor_mismatch", connector, "连接线端点更接近另一个结构锚点，与声明目标不一致", endpoint=endpoint_name, targetId=target_id, nearestElementId=nearest_other[1].get("elementId"), actualPoint=endpoint, expectedPoint=expected, offsetDistance=distance))
                else:
                    hard.append(_issue("connector_endpoint_not_on_node", connector, "连接线端点没有吸附到目标节点或卡片锚点", endpoint=endpoint_name, targetId=target_id, actualPoint=endpoint, expectedPoint=expected, offsetDistance=distance))

        route_points = connector.get("routePoints") or [start, end]
        for text in texts:
            if text.get("role") == "footer" or text.get("allowOverlap"):
                continue
            if any(
                _segment_intersects_box(segment_start, segment_end, text.get("bbox") or {})
                for segment_start, segment_end in zip(route_points, route_points[1:])
            ):
                owner_id = text.get("owner") or text.get("structuralAncestorId")
                if not owner_id and text.get("role") == "label":
                    text_box = text.get("bbox") or {}
                    for target_id in (
                        connector.get("connectorFrom"),
                        connector.get("connectorTo"),
                    ):
                        target = by_id.get(target_id)
                        target_box = (target or {}).get("anchorBox")
                        expanded_target_box = (
                            {
                                "x": float(target_box["x"]) - 6,
                                "y": float(target_box["y"]) - 6,
                                "width": float(target_box["width"]) + 12,
                                "height": float(target_box["height"]) + 12,
                            }
                            if target_box
                            else None
                        )
                        if (
                            target_id
                            and expanded_target_box
                            and _intersection(text_box, expanded_target_box)
                        ):
                            owner_id = target_id
                            break
                hard.append(
                    _issue(
                        "text_line_overlap",
                        text,
                        "连接线穿过可见文字区域",
                        ownerId=owner_id,
                        regionId=text.get("regionId"),
                        collision={"elementId": connector.get("elementId"), "start": start, "end": end},
                    )
                )
        if connector.get("connectorTo") and connector.get("markerStart") and not connector.get("markerEnd"):
            hard.append(_issue("connector_arrow_direction", connector, "箭头位于起点，方向与 data-pome-from/data-pome-to 相反"))

    groups: dict[str, list[dict[str, Any]]] = {}
    for element in elements:
        if element.get("alignGroup") and not composite_container(element):
            groups.setdefault(element["alignGroup"], []).append(element)
    for group_id, members in groups.items():
        if len(members) < 2:
            continue
        axis = next((item.get("alignAxis") for item in members if item.get("alignAxis")), "row")
        values = [item["bbox"]["y"] if axis == "row" else item["bbox"]["x"] for item in members]
        sorted_values = sorted(values)
        target_value = sorted_values[len(sorted_values) // 2]
        widths = sorted(item["bbox"]["width"] for item in members)
        heights = sorted(item["bbox"]["height"] for item in members)
        target_width = widths[len(widths) // 2]
        target_height = heights[len(heights) // 2]
        aligned_peers = sum(abs(value - target_value) <= 2.0 for value in values)
        uniform_size = all(item.get("uniformSize") for item in members)
        for member, value in zip(members, values):
            drift = value - target_value
            width_ratio = abs(member["bbox"]["width"] - target_width) / max(target_width, 1.0)
            height_ratio = abs(member["bbox"]["height"] - target_height) / max(target_height, 1.0)
            is_focal_outlier = aligned_peers >= 2 and max(width_ratio, height_ratio) >= 0.2
            if abs(drift) > 2.0 and not is_focal_outlier:
                hard.append(_issue("grid_alignment_drift", member, "同组元素没有对齐到共同基线或列线", alignGroup=group_id, alignAxis=axis, expectedCoordinate=target_value, offsetDistance=abs(drift)))
            size_drift = max(abs(member["bbox"]["width"] - target_width), abs(member["bbox"]["height"] - target_height))
            if uniform_size and size_drift > 3.0:
                hard.append(_issue("grid_size_drift", member, "同组卡片或节点尺寸不一致", alignGroup=group_id, expectedSize={"width": target_width, "height": target_height}, offsetDistance=size_drift))

    return {
        "schemaVersion": 1,
        "svgPath": measured.get("svgPath"),
        "passed": not hard,
        "hardErrors": hard,
        "warnings": warnings,
        "visualElements": elements,
        "textElements": texts,
        "visibleTexts": [item.get("text", "") for item in texts],
        "measurements": {"elementCount": len(elements), "textCount": len(texts), "cardCount": len(cards), "connectorCount": len(connectors)},
    }


def _local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def _float_attr(element: ET.Element, name: str) -> float:
    return float(element.get(name, "0"))


def _find_id(root: ET.Element, element_id: str) -> ET.Element | None:
    return next((item for item in root.iter() if item.get("id") == element_id), None)


def _find_region_id(root: ET.Element, region_id: str) -> ET.Element | None:
    return next((item for item in root.iter() if item.get("data-pome-region-id") == region_id), None)


def _format_number(value: float) -> str:
    rendered = f"{value:.3f}".rstrip("0").rstrip(".")
    return rendered or "0"


def _shift_native_text_y(element: ET.Element, delta: float) -> None:
    if element.get("y") is not None:
        element.set("y", _format_number(_float_attr(element, "y") + delta))
    if element.get("data-pome-region-y") is not None:
        element.set(
            "data-pome-region-y",
            _format_number(_float_attr(element, "data-pome-region-y") + delta),
        )
    for child in element:
        if _local_name(child.tag) == "tspan" and child.get("y") is not None:
            child.set("y", _format_number(_float_attr(child, "y") + delta))


def deterministic_repair(svg_text: str, report: dict[str, Any]) -> tuple[str, list[dict[str, Any]]]:
    root = ET.fromstring(svg_text)
    applied: list[dict[str, Any]] = []
    elements = {item.get("elementId"): item for item in report.get("visualElements", [])}
    text_elements = {item.get("elementId"): item for item in report.get("textElements", [])}
    resolved_ids: dict[str, str] = {}
    visual_tags = {"g", "rect", "circle", "ellipse", "line", "polyline", "polygon", "path", "text"}
    parents = {child: parent for parent in root.iter() for child in parent}

    def inside_defs(element: ET.Element) -> bool:
        current = parents.get(element)
        while current is not None:
            if _local_name(current.tag) == "defs":
                return True
            current = parents.get(current)
        return False

    visual_elements = [
        element
        for element in root.iter()
        if _local_name(element.tag) in visual_tags
        and not inside_defs(element)
    ]
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "missing_visual_element_id":
            continue
        dom_index = issue.get("domIndex")
        if not isinstance(dom_index, int) or not 0 <= dom_index < len(visual_elements):
            continue
        element = visual_elements[dom_index]
        if element.get("id"):
            continue
        role = (element.get("data-pome-visual-role") or "element").strip().lower()
        element_id = f"pome-auto-{role}-{dom_index + 1}"
        element.set("id", element_id)
        old_id = issue.get("elementId")
        if old_id:
            resolved_ids[old_id] = element_id
            measured = elements.get(old_id)
            if measured is not None:
                elements[element_id] = measured
        applied.append(
            {
                "action": "assign-structural-element-id",
                "elementId": element_id,
                "previousElementId": old_id,
                "role": role,
            }
        )
    endpoint_error_connectors = {
        resolved_ids.get(issue.get("elementId"), issue.get("elementId"))
        for issue in report.get("hardErrors", [])
        if issue.get("rule")
        in {"connector_endpoint_not_on_node", "visual_anchor_mismatch"}
    }
    route_label_boxes: dict[str, list[dict[str, float]]] = {}
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "text_line_overlap" or issue.get("role") != "label":
            continue
        collision = issue.get("collision") or {}
        connector_id = resolved_ids.get(
            collision.get("elementId"), collision.get("elementId")
        )
        connector = elements.get(connector_id) or elements.get(
            collision.get("elementId")
        )
        text = text_elements.get(issue.get("elementId"))
        text_box = (text or {}).get("bbox") or issue.get("actualBounds") or {}
        if (
            not connector_id
            or connector_id in endpoint_error_connectors
            or not connector
            or connector.get("tag") != "line"
            or not text_box
        ):
            continue
        start, end = connector.get("start"), connector.get("end")
        if not start or not end:
            continue
        route_label_boxes.setdefault(connector_id, []).append(text_box)

    rerouted_connectors: set[str] = set()
    for connector_id, label_boxes in route_label_boxes.items():
        element = _find_id(root, connector_id)
        if (
            element is None
            or _local_name(element.tag) != "line"
            or element.get("transform")
        ):
            continue
        start = {"x": _float_attr(element, "x1"), "y": _float_attr(element, "y1")}
        end = {"x": _float_attr(element, "x2"), "y": _float_attr(element, "y2")}
        label_left = min(float(box["x"]) for box in label_boxes)
        label_right = max(_right(box) for box in label_boxes)
        label_top = min(float(box["y"]) for box in label_boxes)
        label_bottom = max(_bottom(box) for box in label_boxes)
        x_candidates = [label_left - 8.0, label_right + 8.0]
        x_candidates = [
            value
            for value in x_candidates
            if 16.0 <= value <= 1264.0
            and min(abs(value - start["x"]), abs(value - end["x"])) <= 120.0
        ]
        y_candidates = [label_top - 8.0, label_bottom + 8.0]
        y_candidates = [
            value
            for value in y_candidates
            if 16.0 <= value <= 704.0
            and min(abs(value - start["y"]), abs(value - end["y"])) <= 120.0
        ]
        route_candidates = [
            (
                "x",
                value,
                [
                    start,
                    {"x": value, "y": start["y"]},
                    {"x": value, "y": end["y"]},
                    end,
                ],
            )
            for value in x_candidates
        ] + [
            (
                "y",
                value,
                [
                    start,
                    {"x": start["x"], "y": value},
                    {"x": end["x"], "y": value},
                    end,
                ],
            )
            for value in y_candidates
        ]
        if not route_candidates:
            continue

        def crossed_text_count(route: list[dict[str, float]]) -> int:
            return sum(
                any(
                    _segment_intersects_box(segment_start, segment_end, text_box)
                    for segment_start, segment_end in zip(route, route[1:])
                )
                for text in text_elements.values()
                if (text_box := text.get("bbox"))
            )

        def route_length(route: list[dict[str, float]]) -> float:
            return sum(
                _point_distance(segment_start, segment_end)
                for segment_start, segment_end in zip(route, route[1:])
            )

        ranked = sorted(
            (
                (
                    crossed_text_count(route),
                    route_length(route),
                    axis,
                    value,
                    route,
                )
                for axis, value, route in route_candidates
            ),
            key=lambda item: (item[0], item[1]),
        )
        if ranked[0][0] > 0:
            continue
        _, _, bypass_axis, bypass_value, route = ranked[0]
        element.tag = f"{{{SVG_NS}}}polyline"
        for attribute in ("x1", "y1", "x2", "y2"):
            element.attrib.pop(attribute, None)
        element.set(
            "points",
            " ".join(
                f'{_format_number(point["x"])},{_format_number(point["y"])}'
                for point in route
            ),
        )
        element.set("fill", "none")
        rerouted_connectors.add(connector_id)
        applied.append(
            {
                "action": "reroute-connector-around-label",
                "elementId": connector_id,
                "bypassAxis": bypass_axis,
                "bypassCoordinate": bypass_value,
                "labelCount": len(label_boxes),
            }
        )
    shifted_text_regions: set[str] = set()
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "text_line_overlap" or issue.get("role") != "label":
            continue
        text = text_elements.get(issue.get("elementId"))
        owner_id = issue.get("ownerId") or (text or {}).get("owner")
        owner = elements.get(owner_id)
        collision = issue.get("collision") or {}
        connector_id = resolved_ids.get(collision.get("elementId"), collision.get("elementId"))
        if connector_id in rerouted_connectors:
            continue
        connector = elements.get(connector_id) or elements.get(collision.get("elementId"))
        region_id = issue.get("regionId") or (text or {}).get("regionId")
        if not text or not owner or not connector or not region_id or region_id in shifted_text_regions:
            continue
        text_box = text.get("bbox") or issue.get("actualBounds") or {}
        owner_box = owner.get("anchorBox") or owner.get("bbox") or {}
        start, end = connector.get("start"), connector.get("end")
        if not start or not end or not text_box or not owner_box:
            continue
        owner_left = float(owner_box.get("x", 0))
        owner_top = float(owner_box.get("y", 0))
        owner_right = owner_left + float(owner_box.get("width", 0))
        owner_bottom = owner_top + float(owner_box.get("height", 0))
        endpoint = end if _point_distance(end, {"x": end["x"], "y": owner_top}) <= 2 else start
        endpoint_x = float(endpoint.get("x", 0))
        endpoint_y = float(endpoint.get("y", 0))
        text_top = float(text_box.get("y", 0))
        text_bottom = text_top + float(text_box.get("height", 0))
        delta = 0.0
        anchor_overlap = (
            _intersection(text_box, owner_box) if owner.get("anchorBox") else None
        )
        if anchor_overlap and issue.get("role") == "label":
            text_center_y = (text_top + text_bottom) / 2
            owner_center_y = (owner_top + owner_bottom) / 2
            delta = (
                owner_top - 4 - text_bottom
                if text_center_y <= owner_center_y
                else owner_bottom + 4 - text_top
            )
        elif owner_left <= endpoint_x <= owner_right and abs(endpoint_y - owner_top) <= 2:
            delta = owner_top + 4 - text_top
            if text_bottom + delta > owner_bottom - 4:
                continue
        elif owner_left <= endpoint_x <= owner_right and abs(endpoint_y - owner_bottom) <= 2:
            delta = owner_bottom - 4 - text_bottom
            if text_top + delta < owner_top + 4:
                delta = 0.0
        if (
            (abs(delta) < 0.5 or abs(delta) > 16)
            and owner.get("anchorBox")
            and abs(float(start["x"]) - float(end["x"])) <= 2
            and owner_id in {connector.get("connectorFrom"), connector.get("connectorTo")}
        ):
            line_x = (float(start["x"]) + float(end["x"])) / 2
            text_left = float(text_box.get("x", 0))
            text_right = text_left + float(text_box.get("width", 0))
            if text_left <= line_x <= text_right:
                candidates = [line_x - 8 - text_right, line_x + 8 - text_left]
                candidates = [
                    value
                    for value in candidates
                    if abs(value) <= 32
                    and text_left + value >= 0
                    and text_right + value <= 1280
                ]
                if candidates:
                    dx = min(candidates, key=lambda value: (abs(value), -value))
                    element = _find_region_id(root, region_id)
                    if element is not None and _local_name(element.tag) == "text" and not element.get("transform"):
                        moved = False
                        if element.get("x") is not None:
                            element.set("x", _format_number(_float_attr(element, "x") + dx))
                            moved = True
                        for child in element:
                            if _local_name(child.tag) == "tspan" and child.get("x") is not None:
                                child.set("x", _format_number(_float_attr(child, "x") + dx))
                                moved = True
                        if moved:
                            if element.get("data-pome-region-x") is not None:
                                element.set(
                                    "data-pome-region-x",
                                    _format_number(_float_attr(element, "data-pome-region-x") + dx),
                                )
                            shifted_text_regions.add(region_id)
                            applied.append(
                                {
                                    "action": "separate-node-label-from-connector",
                                    "elementId": issue.get("elementId"),
                                    "regionId": region_id,
                                    "connectorId": connector_id,
                                    "dx": dx,
                                }
                            )
                            continue
        if abs(delta) < 0.5 or abs(delta) > 16:
            continue
        element = _find_region_id(root, region_id)
        if element is None or _local_name(element.tag) != "text" or element.get("transform"):
            continue
        moved = False
        if element.get("y") is not None:
            element.set("y", _format_number(_float_attr(element, "y") + delta))
            moved = True
        else:
            for child in element:
                if _local_name(child.tag) == "tspan" and child.get("y") is not None:
                    child.set("y", _format_number(_float_attr(child, "y") + delta))
                    moved = True
        if not moved:
            continue
        if element.get("data-pome-region-y") is not None:
            element.set(
                "data-pome-region-y",
                _format_number(_float_attr(element, "data-pome-region-y") + delta),
            )
        shifted_text_regions.add(region_id)
        applied.append(
            {
                "action": "separate-label-from-connector",
                "elementId": issue.get("elementId"),
                "regionId": region_id,
                "connectorId": connector_id,
                "dy": delta,
            }
        )
    shifted_icon_labels: set[str] = set()
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "icon_label_collision" or issue.get("role") != "label":
            continue
        text = text_elements.get(issue.get("elementId"))
        collision = issue.get("collision") or {}
        shape = elements.get(collision.get("elementId"))
        region_id = issue.get("regionId") or (text or {}).get("regionId")
        overlap = collision.get("intersection") or {}
        if not text or not shape or not region_id or region_id in shifted_icon_labels:
            continue
        text_box = text.get("bbox") or issue.get("actualBounds") or {}
        shape_box = shape.get("bbox") or collision.get("bounds") or {}
        if not text_box or not shape_box or max(float(shape_box.get("width", 0)), float(shape_box.get("height", 0))) > 64:
            continue
        text_center_x = float(text_box.get("x", 0)) + float(text_box.get("width", 0)) / 2
        shape_center_x = float(shape_box.get("x", 0)) + float(shape_box.get("width", 0)) / 2
        dx = -(float(overlap.get("width", 0)) + 6) if text_center_x <= shape_center_x else float(overlap.get("width", 0)) + 6
        if abs(dx) < 0.5 or abs(dx) > 20:
            continue
        element = _find_region_id(root, region_id)
        if element is None or _local_name(element.tag) != "text" or element.get("transform"):
            continue
        moved = False
        if element.get("x") is not None:
            element.set("x", _format_number(_float_attr(element, "x") + dx))
            moved = True
        for child in element:
            if _local_name(child.tag) == "tspan" and child.get("x") is not None:
                child.set("x", _format_number(_float_attr(child, "x") + dx))
                moved = True
        if not moved:
            continue
        if element.get("data-pome-region-x") is not None:
            element.set(
                "data-pome-region-x",
                _format_number(_float_attr(element, "data-pome-region-x") + dx),
            )
        shifted_icon_labels.add(region_id)
        applied.append(
            {
                "action": "separate-label-from-icon",
                "elementId": issue.get("elementId"),
                "regionId": region_id,
                "collisionId": collision.get("elementId"),
                "dx": dx,
            }
        )
    group_shifts: dict[str, list[float]] = {}
    repaired_connectors: set[str] = set()
    for issue in report.get("hardErrors", []):
        if issue.get("rule") not in {
            "connector_endpoint_not_on_node",
            "visual_anchor_mismatch",
        }:
            continue
        element_id = resolved_ids.get(issue.get("elementId"), issue.get("elementId"))
        connector = _find_id(root, element_id) if element_id else None
        measured = elements.get(element_id)
        expected = issue.get("expectedPoint")
        connector_tag = _local_name(connector.tag) if connector is not None else ""
        if connector is None or measured is None or expected is None or connector_tag not in {"line", "polyline"} or connector.get("transform"):
            continue
        endpoint = issue.get("endpoint")
        if connector_tag == "line" and endpoint == "start":
            connector.set("x1", _format_number(float(expected["x"])))
            connector.set("y1", _format_number(float(expected["y"])))
        elif connector_tag == "line" and endpoint == "end":
            connector.set("x2", _format_number(float(expected["x"])))
            connector.set("y2", _format_number(float(expected["y"])))
        elif connector_tag == "polyline" and endpoint in {"start", "end"}:
            points = [
                [float(value) for value in token.replace(",", " ").split()]
                for token in connector.get("points", "").split()
            ]
            points = [point for point in points if len(point) == 2]
            if len(points) < 2:
                continue
            index = 0 if endpoint == "start" else -1
            points[index] = [float(expected["x"]), float(expected["y"])]
            connector.set(
                "points",
                " ".join(
                    f"{_format_number(point[0])},{_format_number(point[1])}"
                    for point in points
                ),
            )
        else:
            continue
        repaired_connectors.add(element_id)
        applied.append({"action": "snap-connector-endpoint", "elementId": element_id, "endpoint": endpoint, "targetId": issue.get("targetId"), "offsetDistance": issue.get("offsetDistance")})

    connector_targets = {
        target_id
        for connector in elements.values()
        if connector.get("tag") in {"line", "polyline", "path"}
        for target_id in (connector.get("connectorFrom"), connector.get("connectorTo"))
        if target_id
    }
    vertically_shifted_cards: set[str] = set()
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "card_card_overlap":
            continue
        collision = issue.get("collision") or {}
        first = elements.get(issue.get("elementId"))
        second = elements.get(collision.get("elementId"))
        overlap = collision.get("intersection") or {}
        if not first or not second:
            continue
        overlap_width = float(overlap.get("width", 0))
        overlap_height = float(overlap.get("height", 0))
        if not 0 < overlap_height <= 12:
            continue
        if overlap_width < min(
            float(first["bbox"]["width"]), float(second["bbox"]["width"])
        ) * 0.45:
            continue
        upper, lower = sorted(
            (first, second), key=lambda item: float(item["bbox"]["y"])
        )
        card_id = lower.get("elementId")
        delta = overlap_height + 8.0
        if (
            not card_id
            or card_id in vertically_shifted_cards
            or card_id in connector_targets
            or lower.get("tag") != "rect"
            or _bottom(lower["bbox"]) + delta > CANVAS["height"] - 28.0
        ):
            continue
        element = _find_id(root, card_id)
        if (
            element is None
            or _local_name(element.tag) != "rect"
            or element.get("transform")
            or element.get("y") is None
        ):
            continue
        owned_text_ids = [
            text.get("regionId")
            for text in text_elements.values()
            if text.get("owner") == card_id and text.get("regionId")
        ]
        owned_text_nodes = [
            _find_region_id(root, region_id) for region_id in owned_text_ids
        ]
        if any(
            node is None
            or _local_name(node.tag) != "text"
            or node.get("transform")
            for node in owned_text_nodes
        ):
            continue
        element.set("y", _format_number(_float_attr(element, "y") + delta))
        for text_node in owned_text_nodes:
            _shift_native_text_y(text_node, delta)
        vertically_shifted_cards.add(card_id)
        applied.append(
            {
                "action": "separate-vertically-overlapping-card",
                "elementId": card_id,
                "collisionId": upper.get("elementId"),
                "dy": delta,
            }
        )

    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "grid_alignment_drift" or issue.get("offsetDistance", 99) > 6:
            continue
        element_id = issue.get("elementId")
        element = _find_id(root, element_id) if element_id else None
        measured = elements.get(element_id)
        if element is None or measured is None or _local_name(element.tag) != "g" or element.get("transform"):
            continue
        axis = issue.get("alignAxis")
        current = measured["bbox"]["y" if axis == "row" else "x"]
        delta = float(issue["expectedCoordinate"]) - float(current)
        dx, dy = (0.0, delta) if axis == "row" else (delta, 0.0)
        shift = group_shifts.setdefault(element_id, [0.0, 0.0])
        shift[0] += dx
        shift[1] += dy
        applied.append({"action": "align-structural-group", "elementId": element_id, "alignGroup": issue.get("alignGroup"), "dx": dx, "dy": dy})
    resized_cards: set[str] = set()
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "card_card_overlap":
            continue
        collision = issue.get("collision") or {}
        first = elements.get(issue.get("elementId"))
        second = elements.get(collision.get("elementId"))
        overlap = collision.get("intersection") or {}
        if not first or not second or first.get("tag") != "rect" or second.get("tag") != "rect":
            continue
        if first.get("role") != "card" or second.get("role") != "card":
            continue
        left, right = sorted((first, second), key=lambda item: float(item["bbox"]["x"]))
        left_id = left.get("elementId")
        overlap_width = float(overlap.get("width", 0))
        overlap_height = float(overlap.get("height", 0))
        min_height = min(float(left["bbox"]["height"]), float(right["bbox"]["height"]))
        if not left_id or left_id in resized_cards or not 0 < overlap_width <= 40:
            continue
        if overlap_height < min_height * 0.45:
            continue
        element = _find_id(root, left_id)
        if element is None or element.get("transform") or element.get("width") is None:
            continue
        new_width = float(left["bbox"]["width"]) - overlap_width - 8.0
        if new_width < 120:
            continue
        new_right = float(left["bbox"]["x"]) + new_width
        owned_texts = [
            item for item in text_elements.values() if item.get("owner") == left_id
        ]
        if any(_right(item["bbox"]) > new_right - 4.0 for item in owned_texts):
            continue
        element.set("width", _format_number(new_width))
        left["bbox"]["width"] = new_width
        resized_cards.add(left_id)
        applied.append(
            {
                "action": "shrink-overlapping-card-right-edge",
                "elementId": left_id,
                "previousWidth": left["bbox"]["width"],
                "width": new_width,
            }
        )
    repaired_connectors.update(
        element_id
        for element_id, item in elements.items()
        if element_id
        and item.get("tag") == "line"
        and item.get("connectorFrom")
        and item.get("connectorTo")
    )
    # Snapping both ends against their original opposite endpoint can leave a
    # small residual error on circles/ellipses. Recompute the pair together so
    # both current anchors converge after any local card resize.
    for element_id in repaired_connectors:
        connector = _find_id(root, element_id)
        measured = elements.get(element_id)
        if connector is None or measured is None or _local_name(connector.tag) != "line":
            continue
        source = elements.get(measured.get("connectorFrom"))
        target = elements.get(measured.get("connectorTo"))
        if source is None or target is None:
            continue
        for _ in range(4):
            start = {"x": _float_attr(connector, "x1"), "y": _float_attr(connector, "y1")}
            end = {"x": _float_attr(connector, "x2"), "y": _float_attr(connector, "y2")}
            snapped_start = _nearest_anchor(source, end)
            snapped_end = _nearest_anchor(target, snapped_start)
            connector.set("x1", _format_number(snapped_start["x"]))
            connector.set("y1", _format_number(snapped_start["y"]))
            connector.set("x2", _format_number(snapped_end["x"]))
            connector.set("y2", _format_number(snapped_end["y"]))
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "card_card_overlap":
            continue
        left = elements.get(issue.get("elementId"))
        right = elements.get((issue.get("collision") or {}).get("elementId"))
        if not left or not right or left.get("tag") != "g" or right.get("tag") != "g":
            continue
        overlap = (issue.get("collision") or {}).get("intersection") or {}
        same_alignment_group = bool(left.get("alignGroup")) and (
            left.get("alignGroup") == right.get("alignGroup")
        )
        axis = right.get("alignAxis") or left.get("alignAxis") or "row"
        if not same_alignment_group:
            overlap_width = float(overlap.get("width", 0))
            overlap_height = float(overlap.get("height", 0))
            min_height = min(
                float(left["bbox"]["height"]), float(right["bbox"]["height"])
            )
            if (
                left.get("role") != "card"
                or right.get("role") != "card"
                or not 0 < overlap_width <= 12
                or overlap_height < min_height * 0.45
            ):
                continue
            axis = "row"
        distance = float(
            overlap.get("width" if axis == "row" else "height", 0)
        ) + 8.0
        if distance <= 0 or distance > 16:
            continue
        first, second = sorted(
            (left, right),
            key=lambda item: (
                float(item["bbox"]["x"]), float(item["bbox"]["y"])
            ),
        )
        candidates = (
            ((second, distance, 0.0), (first, -distance, 0.0))
            if axis == "row"
            else ((second, 0.0, distance), (first, 0.0, -distance))
        )
        target = None
        dx = dy = 0.0
        explicit_group_cards = [
            item
            for item in elements.values()
            if item.get("tag") == "g" and item.get("role") == "card"
        ]
        for candidate, candidate_dx, candidate_dy in candidates:
            candidate_element = _find_id(root, candidate.get("elementId"))
            if candidate_element is None or candidate_element.get("transform"):
                continue
            shifted = dict(candidate["bbox"])
            shifted["x"] = float(shifted["x"]) + candidate_dx
            shifted["y"] = float(shifted["y"]) + candidate_dy
            if (
                shifted["x"] < 28
                or shifted["y"] < 20
                or _right(shifted) > CANVAS["width"] - 28
                or _bottom(shifted) > CANVAS["height"] - 28
            ):
                continue
            collides_elsewhere = any(
                other.get("elementId")
                not in {left.get("elementId"), right.get("elementId")}
                and (intersection := _intersection(shifted, other["bbox"]))
                is not None
                and _area(intersection) >= 36
                for other in explicit_group_cards
            )
            if collides_elsewhere:
                continue
            target, dx, dy = candidate, candidate_dx, candidate_dy
            break
        if target is None:
            continue
        shift = group_shifts.setdefault(target["elementId"], [0.0, 0.0])
        shift[0] += dx
        shift[1] += dy
        applied.append({"action": "separate-overlapping-card-group", "elementId": target["elementId"], "alignGroup": target.get("alignGroup"), "dx": dx, "dy": dy})
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "element_outside_safe_area":
            continue
        element_id = issue.get("elementId")
        element = _find_id(root, element_id) if element_id else None
        measured = elements.get(element_id)
        overflow = issue.get("overflow") or {}
        if element is None or measured is None or _local_name(element.tag) != "g" or element.get("transform"):
            continue
        dx = float(overflow.get("left", 0)) - float(overflow.get("right", 0))
        dy = float(overflow.get("top", 0)) - float(overflow.get("bottom", 0))
        if max(abs(dx), abs(dy)) > 12:
            continue
        shift = group_shifts.setdefault(element_id, [0.0, 0.0])
        shift[0] += dx
        shift[1] += dy
        applied.append({"action": "move-structural-group-inside-canvas", "elementId": element_id, "dx": dx, "dy": dy})
    for element_id, (dx, dy) in group_shifts.items():
        element = _find_id(root, element_id)
        if element is not None and not element.get("transform"):
            element.set("transform", f"translate({_format_number(dx)} {_format_number(dy)})")
    for issue in report.get("hardErrors", []):
        if issue.get("rule") != "connector_arrow_direction":
            continue
        element_id = issue.get("elementId")
        element = _find_id(root, element_id) if element_id else None
        if element is None or _local_name(element.tag) != "line":
            continue
        marker = element.get("marker-start")
        if not marker or element.get("marker-end"):
            continue
        element.attrib.pop("marker-start", None)
        element.set("marker-end", marker)
        applied.append({"action": "correct-connector-arrow-direction", "elementId": element_id})
    if not applied:
        return svg_text, applied
    return ET.tostring(root, encoding="unicode"), applied


def _measure(session: Any, svg_path: Path) -> dict[str, Any]:
    text_measurements = measure_svg(session, svg_path)
    result = session.command("Runtime.evaluate", {"expression": VISUAL_MEASURE_SCRIPT, "awaitPromise": True, "returnByValue": True})
    elements = result.get("result", {}).get("value")
    if not isinstance(elements, list):
        raise RuntimeError("浏览器未返回视觉元素测量结果")
    texts = text_measurements.get("texts", [])
    text_by_index = {item.get("domIndex"): item for item in texts}
    for element in elements:
        if element.get("tag") == "text":
            matching = next((item for item in texts if item.get("text") == element.get("text") and abs(item.get("bbox", {}).get("x", 0) - element.get("bbox", {}).get("x", 0)) < 1), None)
            if matching:
                element["textGeometryId"] = matching.get("elementId")
                matching["owner"] = element.get("owner")
                matching["structuralAncestorId"] = element.get("structuralAncestorId")
    return {"svgPath": str(svg_path), "elements": elements, "texts": texts, "textByIndex": text_by_index}


def run(svg_path: Path, auto_fix: bool = False, require_contract: bool = False) -> dict[str, Any]:
    original = svg_path.read_text(encoding="utf-8")
    session = open_browser()
    try:
        before = classify_measurements(_measure(session, svg_path), require_contract=require_contract)
        preflight_applied: list[dict[str, Any]] = []
        if auto_fix:
            missing_id_errors = [
                issue
                for issue in before.get("hardErrors", [])
                if issue.get("rule") == "missing_visual_element_id"
            ]
            if missing_id_errors:
                id_report = dict(before)
                id_report["hardErrors"] = missing_id_errors
                identified, id_applied = deterministic_repair(original, id_report)
                if id_applied:
                    with tempfile.NamedTemporaryFile(
                        "w",
                        suffix=".svg",
                        encoding="utf-8",
                        delete=False,
                        dir=svg_path.parent,
                    ) as handle:
                        handle.write(identified)
                        identified_path = Path(handle.name)
                    try:
                        identified_report = classify_measurements(
                            _measure(session, identified_path),
                            require_contract=require_contract,
                        )
                    finally:
                        identified_path.unlink(missing_ok=True)
                    if before.get("visibleTexts", []) == identified_report.get(
                        "visibleTexts", []
                    ):
                        original = identified
                        svg_path.write_text(original, encoding="utf-8")
                        before = identified_report
                        preflight_applied.extend(id_applied)
        before["autoFixApplied"] = list(preflight_applied)
        if not auto_fix or before["passed"]:
            return before
        candidate, applied = deterministic_repair(original, before)
        if not applied:
            before["failureKind"] = "page_relayout_required"
            return before
        before_text = before.get("visibleTexts", [])
        all_applied = list(preflight_applied) + list(applied)
        previous_report = before
        after = before
        # A connector snap can expose a label collision that did not exist on
        # the malformed route.  Re-measure and allow two bounded deterministic
        # follow-up passes so the label can be separated transactionally.
        for repair_round in range(3):
            with tempfile.NamedTemporaryFile("w", suffix=".svg", encoding="utf-8", delete=False, dir=svg_path.parent) as handle:
                handle.write(candidate)
                candidate_path = Path(handle.name)
            try:
                after = classify_measurements(_measure(session, candidate_path), require_contract=require_contract)
            finally:
                candidate_path.unlink(missing_ok=True)
            if before_text != after.get("visibleTexts", []):
                break
            if after["passed"]:
                break
            if len(after.get("hardErrors", [])) >= len(previous_report.get("hardErrors", [])):
                break
            if repair_round == 2:
                break
            next_candidate, next_applied = deterministic_repair(candidate, after)
            if not next_applied:
                break
            candidate = next_candidate
            all_applied.extend(next_applied)
            previous_report = after
        applied = all_applied
        after_text = after.get("visibleTexts", [])
        before_element_ids = {
            item.get("elementId")
            for item in before.get("visualElements", [])
            if item.get("elementId")
        }
        renamed_ids = {
            item.get("elementId"): item.get("previousElementId")
            for item in applied
            if item.get("action") == "assign-structural-element-id"
            and item.get("elementId")
            and item.get("previousElementId")
            and item.get("previousElementId") in before_element_ids
        }
        before_signatures = {(item.get("rule"), item.get("elementId")) for item in before.get("hardErrors", [])}
        after_signatures = {
            (item.get("rule"), renamed_ids.get(item.get("elementId"), item.get("elementId")))
            for item in after.get("hardErrors", [])
        }
        introduced = after_signatures - before_signatures
        if before_text != after_text or introduced or len(after.get("hardErrors", [])) >= len(before.get("hardErrors", [])):
            before["failureKind"] = "page_relayout_required"
            before["autoFixRejected"] = {"visibleTextChanged": before_text != after_text, "introducedIssues": sorted(introduced), "remainingHardErrors": len(after.get("hardErrors", []))}
            return before
        svg_path.write_text(candidate, encoding="utf-8")
        after["svgPath"] = str(svg_path)
        after["autoFixApplied"] = applied
        if not after["passed"]:
            after["failureKind"] = "page_relayout_required"
        return after
    finally:
        session.close()


class VisualDetailClassificationTests(unittest.TestCase):
    def measured(self, elements: list[dict[str, Any]], texts: list[dict[str, Any]] | None = None) -> dict[str, Any]:
        return {"svgPath": "test.svg", "elements": elements, "texts": texts or []}

    def test_composite_timeline_group_does_not_collide_with_its_card_text(self) -> None:
        elements = [
            {"elementId": "timeline-item", "hasExplicitId": True, "domIndex": 0, "tag": "g", "role": "node", "structuralDescendantCount": 2, "bbox": {"x": 80, "y": 170, "width": 230, "height": 205}},
            {"elementId": "card", "hasExplicitId": True, "domIndex": 1, "tag": "rect", "role": "card", "bbox": {"x": 88, "y": 176, "width": 224, "height": 94}},
        ]
        texts = [
            {"elementId": "label", "domIndex": 0, "role": "label", "owner": "card", "regionId": "card-label", "region": {"x": 88, "y": 170, "width": 224, "height": 22}, "bbox": {"x": 102, "y": 171, "width": 106, "height": 19}},
        ]

        report = classify_measurements(self.measured(elements, texts))

        self.assertTrue(report["passed"], report)

    def test_composite_card_uses_panel_anchor_and_dedupes_child_surface(self) -> None:
        elements = [
            {"elementId": "card-a", "hasExplicitId": True, "domIndex": 0, "tag": "g", "role": "card", "bbox": {"x": 100, "y": 180, "width": 196, "height": 158}, "anchorBox": {"x": 100, "y": 180, "width": 196, "height": 112}},
            {"elementId": "surface-a", "hasExplicitId": False, "domIndex": 1, "tag": "rect", "role": "", "bbox": {"x": 100, "y": 180, "width": 196, "height": 112}},
            {"elementId": "card-b", "hasExplicitId": True, "domIndex": 2, "tag": "g", "role": "card", "bbox": {"x": 320, "y": 180, "width": 196, "height": 158}, "anchorBox": {"x": 320, "y": 180, "width": 196, "height": 112}},
            {"elementId": "surface-b", "hasExplicitId": False, "domIndex": 3, "tag": "rect", "role": "", "bbox": {"x": 320, "y": 180, "width": 196, "height": 112}},
        ]
        texts = [
            {"bbox": {"x": 116, "y": 200, "width": 80, "height": 20}},
            {"bbox": {"x": 336, "y": 200, "width": 80, "height": 20}},
        ]

        cards = _infer_cards(elements, texts)

        self.assertEqual([item["elementId"] for item in cards], ["card-a", "card-b"])
        self.assertEqual(cards[0]["bbox"], elements[0]["anchorBox"])

    def test_line_anchor_projects_to_nearest_point_on_timeline_axis(self) -> None:
        axis = {
            "tag": "line",
            "bbox": {"x": 100, "y": 370, "width": 1000, "height": 0},
            "routePoints": [{"x": 100, "y": 370}, {"x": 1100, "y": 370}],
        }
        self.assertEqual(_nearest_anchor(axis, {"x": 320, "y": 480}), {"x": 320.0, "y": 370.0})

    def test_composite_timeline_node_uses_inner_circle_anchor_not_group_bounds(self) -> None:
        node = {
            "tag": "g",
            "role": "node",
            "bbox": {"x": 210, "y": 198, "width": 160, "height": 220},
            "anchorBox": {"x": 252, "y": 332, "width": 76, "height": 76},
            "anchorShape": "circle",
        }

        self.assertEqual(_nearest_anchor(node, {"x": 290, "y": 272}), {"x": 290.0, "y": 332.0})

    def test_label_fully_inside_sibling_node_is_not_an_icon_collision(self) -> None:
        elements = [
            {"elementId": "family-card", "hasExplicitId": True, "domIndex": 0, "tag": "rect", "role": "card", "bbox": {"x": 100, "y": 100, "width": 500, "height": 300}},
            {"elementId": "person-node", "hasExplicitId": True, "domIndex": 1, "tag": "rect", "role": "node", "bbox": {"x": 240, "y": 180, "width": 120, "height": 40}},
        ]
        texts = [
            {"elementId": "text-1", "domIndex": 0, "role": "label", "owner": "family-card", "regionId": "person-label", "region": {"x": 242, "y": 182, "width": 116, "height": 36}, "bbox": {"x": 270, "y": 190, "width": 60, "height": 19}},
        ]

        report = classify_measurements(self.measured(elements, texts))

        self.assertNotIn(
            "icon_label_collision",
            {item["rule"] for item in report["hardErrors"]},
        )

    def test_short_unresolved_leader_is_decorative_not_a_broken_connector(self) -> None:
        elements = [
            {"elementId": "node", "hasExplicitId": True, "domIndex": 0, "tag": "circle", "role": "node", "bbox": {"x": 75, "y": 62, "width": 10, "height": 10}},
            {"elementId": "visual-2", "hasExplicitId": False, "domIndex": 1, "tag": "line", "role": "connector", "bbox": {"x": 92, "y": 67, "width": 16, "height": 0}, "start": {"x": 92, "y": 67}, "end": {"x": 108, "y": 67}, "routePoints": [{"x": 92, "y": 67}, {"x": 108, "y": 67}], "connectorFrom": "node", "connectorTo": "title-region", "markerStart": None, "markerEnd": None},
        ]

        report = classify_measurements(self.measured(elements))

        self.assertTrue(report["passed"], report)

    def test_connector_to_composite_decoration_does_not_require_semantic_anchor(self) -> None:
        elements = [
            {"elementId": "portrait", "hasExplicitId": True, "domIndex": 0, "tag": "g", "role": "decoration", "decorative": True, "structuralDescendantCount": 3, "bbox": {"x": 816, "y": 190, "width": 288, "height": 430}},
            {"elementId": "card", "hasExplicitId": True, "domIndex": 1, "tag": "rect", "role": "card", "bbox": {"x": 340, "y": 340, "width": 280, "height": 102}},
            {"elementId": "leader", "hasExplicitId": True, "domIndex": 2, "tag": "line", "role": "connector", "connectorFrom": "portrait", "connectorTo": "card", "start": {"x": 820, "y": 391}, "end": {"x": 620, "y": 391}, "routePoints": [{"x": 820, "y": 391}, {"x": 620, "y": 391}], "bbox": {"x": 620, "y": 391, "width": 200, "height": 0}},
        ]

        report = classify_measurements(self.measured(elements))

        self.assertTrue(report["passed"], report)

    def test_missing_structural_id_is_assigned_without_changing_geometry(self) -> None:
        svg = f'<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720"><line x1="100" y1="100" x2="200" y2="100" data-pome-visual-role="connector"/></svg>'
        report = {
            "visualElements": [{"elementId": "visual-1", "domIndex": 0, "tag": "line", "bbox": {"x": 100, "y": 100, "width": 100, "height": 0}}],
            "hardErrors": [{"rule": "missing_visual_element_id", "elementId": "visual-1", "domIndex": 0, "role": "connector"}],
        }

        repaired, applied = deterministic_repair(svg, report)

        self.assertIn('id="pome-auto-connector-1"', repaired)
        self.assertIn('x1="100"', repaired)
        self.assertTrue(any(item["action"] == "assign-structural-element-id" for item in applied))

    def test_label_on_card_edge_moves_inward_without_changing_text(self) -> None:
        svg = f'''<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720">
        <rect id="card" x="548" y="480" width="224" height="94" data-pome-visual-role="card"/>
        <line id="connector" x1="660" y1="370" x2="660" y2="480" data-pome-visual-role="connector"/>
        <text data-pome-role="label" data-pome-region-id="card-year" data-pome-region-y="474.5" data-pome-owner="card" x="562" y="490">1936 · 延安时期</text>
        </svg>'''
        report = {
            "visualElements": [
                {"elementId": "card", "tag": "rect", "bbox": {"x": 548, "y": 480, "width": 224, "height": 94}},
                {"elementId": "connector", "tag": "line", "start": {"x": 660, "y": 370}, "end": {"x": 660, "y": 480}, "bbox": {"x": 660, "y": 370, "width": 0, "height": 110}},
            ],
            "textElements": [
                {"elementId": "text-1", "role": "label", "owner": "card", "regionId": "card-year", "bbox": {"x": 562, "y": 475, "width": 105, "height": 19}},
            ],
            "hardErrors": [
                {"rule": "text_line_overlap", "elementId": "text-1", "role": "label", "ownerId": "card", "regionId": "card-year", "actualBounds": {"x": 562, "y": 475, "width": 105, "height": 19}, "collision": {"elementId": "connector", "start": {"x": 660, "y": 370}, "end": {"x": 660, "y": 480}}},
            ],
        }

        repaired, applied = deterministic_repair(svg, report)

        root = ET.fromstring(repaired)
        label = _find_region_id(root, "card-year")
        self.assertIsNotNone(label)
        self.assertEqual(label.text, "1936 · 延安时期")
        self.assertEqual(label.get("y"), "499")
        self.assertEqual(label.get("data-pome-region-y"), "483.5")
        self.assertTrue(any(item["action"] == "separate-label-from-connector" for item in applied))

    def test_small_timeline_label_collision_moves_label_away_from_node(self) -> None:
        svg = f'''<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720">
        <circle id="node" cx="692" cy="286" r="6" data-pome-visual-role="node"/>
        <text data-pome-role="label" data-pome-region-id="timeline-label" data-pome-region-x="620" x="620" y="283"><tspan x="620" y="283">1949 开国大典</tspan></text>
        </svg>'''
        report = {
            "visualElements": [
                {"elementId": "node", "tag": "circle", "role": "node", "bbox": {"x": 686, "y": 280, "width": 12, "height": 12}},
            ],
            "textElements": [
                {"elementId": "text-1", "role": "label", "regionId": "timeline-label", "bbox": {"x": 620, "y": 270, "width": 79.7, "height": 16}},
            ],
            "hardErrors": [
                {"rule": "icon_label_collision", "elementId": "text-1", "role": "label", "regionId": "timeline-label", "actualBounds": {"x": 620, "y": 270, "width": 79.7, "height": 16}, "collision": {"elementId": "node", "bounds": {"x": 686, "y": 280, "width": 12, "height": 12}, "intersection": {"x": 686, "y": 280, "width": 12, "height": 6}}},
            ],
        }

        repaired, applied = deterministic_repair(svg, report)

        root = ET.fromstring(repaired)
        label = _find_region_id(root, "timeline-label")
        self.assertIsNotNone(label)
        self.assertEqual("".join(label.itertext()), "1949 开国大典")
        self.assertEqual(label.get("x"), "602")
        self.assertEqual(label.find(f"{{{SVG_NS}}}tspan").get("x"), "602")
        self.assertTrue(any(item["action"] == "separate-label-from-icon" for item in applied))

    def test_card_overlap_is_hard_with_explicit_contract(self) -> None:
        elements = [
            {"elementId": "a", "hasExplicitId": True, "domIndex": 0, "tag": "rect", "role": "card", "bbox": {"x": 100, "y": 100, "width": 220, "height": 120}},
            {"elementId": "b", "hasExplicitId": True, "domIndex": 1, "tag": "rect", "role": "card", "bbox": {"x": 300, "y": 110, "width": 220, "height": 120}},
        ]
        report = classify_measurements(self.measured(elements))
        self.assertTrue(any(item["rule"] == "card_card_overlap" for item in report["hardErrors"]))

    def test_explicit_card_group_deduplicates_its_inferred_background_rect(self) -> None:
        elements = [
            {"elementId": "card-a", "hasExplicitId": True, "domIndex": 0, "tag": "g", "role": "card", "bbox": {"x": 100, "y": 100, "width": 200, "height": 120}},
            {"elementId": "visual-a", "hasExplicitId": False, "domIndex": 1, "tag": "rect", "role": "", "decorative": False, "bbox": {"x": 100, "y": 100, "width": 200, "height": 120}},
            {"elementId": "card-b", "hasExplicitId": True, "domIndex": 2, "tag": "g", "role": "card", "bbox": {"x": 295, "y": 100, "width": 200, "height": 120}},
            {"elementId": "visual-b", "hasExplicitId": False, "domIndex": 3, "tag": "rect", "role": "", "decorative": False, "bbox": {"x": 295, "y": 100, "width": 200, "height": 120}},
        ]
        texts = [
            {"elementId": "text-a", "text": "A", "bbox": {"x": 120, "y": 120, "width": 20, "height": 20}},
            {"elementId": "text-b", "text": "B", "bbox": {"x": 315, "y": 120, "width": 20, "height": 20}},
        ]

        report = classify_measurements(self.measured(elements, texts))

        overlaps = [
            item for item in report["hardErrors"] if item["rule"] == "card_card_overlap"
        ]
        self.assertEqual(report["measurements"]["cardCount"], 2)
        self.assertEqual(len(overlaps), 1)
        self.assertEqual(overlaps[0]["elementId"], "card-a")
        self.assertEqual(overlaps[0]["collision"]["elementId"], "card-b")

    def test_semantic_shape_overlap_and_label_region_are_distinct(self) -> None:
        elements = [
            {"elementId": "card", "hasExplicitId": True, "domIndex": 0, "tag": "rect", "role": "card", "bbox": {"x": 100, "y": 100, "width": 220, "height": 120}},
            {"elementId": "node-a", "hasExplicitId": True, "domIndex": 1, "tag": "circle", "role": "node", "bbox": {"x": 350, "y": 100, "width": 40, "height": 40}},
            {"elementId": "icon-b", "hasExplicitId": True, "domIndex": 2, "tag": "rect", "role": "icon", "bbox": {"x": 370, "y": 110, "width": 30, "height": 30}},
        ]
        texts = [
            {"elementId": "label", "domIndex": 0, "role": "label", "owner": "card", "text": "标签", "bbox": {"x": 94, "y": 130, "width": 44, "height": 20}},
        ]
        report = classify_measurements(self.measured(elements, texts))
        rules = {item["rule"] for item in report["hardErrors"]}
        self.assertIn("shape_shape_overlap", rules)
        self.assertIn("label_outside_region", rules)

    def test_connector_declared_target_mismatch_is_classified(self) -> None:
        elements = [
            {"elementId": "n1", "hasExplicitId": True, "domIndex": 0, "tag": "circle", "role": "node", "bbox": {"x": 90, "y": 90, "width": 20, "height": 20}},
            {"elementId": "n2", "hasExplicitId": True, "domIndex": 1, "tag": "circle", "role": "node", "bbox": {"x": 190, "y": 90, "width": 20, "height": 20}},
            {"elementId": "n3", "hasExplicitId": True, "domIndex": 2, "tag": "rect", "role": "node", "bbox": {"x": 400, "y": 70, "width": 100, "height": 60}},
            {"elementId": "c1", "hasExplicitId": True, "domIndex": 3, "tag": "line", "role": "connector", "connectorFrom": "n1", "connectorTo": "n3", "bbox": {"x": 210, "y": 100, "width": 190, "height": 0}, "start": {"x": 210, "y": 100}, "end": {"x": 400, "y": 100}},
        ]
        report = classify_measurements(self.measured(elements))
        self.assertTrue(any(item["rule"] == "visual_anchor_mismatch" for item in report["hardErrors"]))

    def test_connector_can_use_text_region_and_polyline_endpoint_is_repaired(self) -> None:
        elements = [
            {"elementId": "node", "hasExplicitId": True, "domIndex": 0, "tag": "circle", "role": "node", "bbox": {"x": 878, "y": 258, "width": 124, "height": 124}},
            {"elementId": "route", "hasExplicitId": True, "domIndex": 1, "tag": "polyline", "role": "connector", "connectorFrom": "hero-title", "connectorTo": "node", "bbox": {"x": 520, "y": 250, "width": 370, "height": 55}, "start": {"x": 520, "y": 250}, "end": {"x": 890, "y": 305}, "routePoints": [{"x": 520, "y": 250}, {"x": 580, "y": 250}, {"x": 890, "y": 305}]},
        ]
        texts = [
            {"elementId": "text-1", "regionId": "hero-title", "region": {"x": 48, "y": 148, "width": 520, "height": 137}, "bbox": {"x": 48, "y": 149, "width": 260, "height": 136}, "role": "title", "text": "标题"},
        ]

        report = classify_measurements(self.measured(elements, texts))

        endpoint_errors = [
            item
            for item in report["hardErrors"]
            if item["rule"] == "connector_endpoint_not_on_node"
        ]
        self.assertEqual(len(endpoint_errors), 1)
        self.assertEqual(endpoint_errors[0]["endpoint"], "end")
        svg = f'<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720"><circle id="node" cx="940" cy="320" r="62"/><polyline id="route" points="520,250 580,250 890,305" data-pome-visual-role="connector" data-pome-from="hero-title" data-pome-to="node"/></svg>'
        repaired, applied = deterministic_repair(svg, report)
        route = _find_id(ET.fromstring(repaired), "route")
        self.assertNotEqual(route.get("points").split()[-1], "890,305")
        self.assertTrue(any(item["action"] == "snap-connector-endpoint" for item in applied))

    def test_connector_on_any_declared_card_edge_is_a_valid_anchor(self) -> None:
        elements = [
            {"elementId": "a", "hasExplicitId": True, "domIndex": 0, "tag": "g", "role": "card", "bbox": {"x": 100, "y": 100, "width": 200, "height": 100}},
            {"elementId": "b", "hasExplicitId": True, "domIndex": 1, "tag": "g", "role": "card", "bbox": {"x": 100, "y": 240, "width": 200, "height": 100}},
            {"elementId": "c", "hasExplicitId": True, "domIndex": 2, "tag": "line", "role": "connector", "connectorFrom": "a", "connectorTo": "b", "bbox": {"x": 300, "y": 200, "width": 0, "height": 40}, "start": {"x": 300, "y": 200}, "end": {"x": 300, "y": 240}, "routePoints": [{"x": 300, "y": 200}, {"x": 300, "y": 240}]},
        ]
        report = classify_measurements(self.measured(elements))
        rules = {item["rule"] for item in report["hardErrors"]}
        self.assertNotIn("connector_endpoint_not_on_node", rules)
        self.assertNotIn("visual_anchor_mismatch", rules)

    def test_long_axis_supported_by_three_nodes_does_not_need_pair_endpoints(self) -> None:
        elements = [
            {"elementId": "axis", "hasExplicitId": True, "domIndex": 0, "tag": "line", "role": "connector", "connectorFrom": "missing-start", "connectorTo": "missing-end", "bbox": {"x": 100, "y": 300, "width": 800, "height": 0}, "start": {"x": 100, "y": 300}, "end": {"x": 900, "y": 300}, "routePoints": [{"x": 100, "y": 300}, {"x": 900, "y": 300}]},
            {"elementId": "n1", "hasExplicitId": True, "domIndex": 1, "tag": "circle", "role": "node", "bbox": {"x": 190, "y": 290, "width": 20, "height": 20}},
            {"elementId": "n2", "hasExplicitId": True, "domIndex": 2, "tag": "circle", "role": "node", "bbox": {"x": 490, "y": 290, "width": 20, "height": 20}},
            {"elementId": "n3", "hasExplicitId": True, "domIndex": 3, "tag": "circle", "role": "node", "bbox": {"x": 790, "y": 290, "width": 20, "height": 20}},
        ]
        report = classify_measurements(self.measured(elements))
        self.assertNotIn(
            "connector_contract_incomplete",
            {item["rule"] for item in report["hardErrors"]},
        )
        self.assertNotIn(
            "connector_endpoint_not_on_node",
            {item["rule"] for item in report["hardErrors"]},
        )

    def test_composite_axis_supported_by_nodes_does_not_need_pair_endpoints(self) -> None:
        elements = [
            {"elementId": "axis", "hasExplicitId": True, "domIndex": 0, "tag": "g", "role": "connector", "bbox": {"x": 48, "y": 390, "width": 1184, "height": 10}},
            {"elementId": "n1", "hasExplicitId": True, "domIndex": 1, "tag": "g", "role": "node", "bbox": {"x": 44, "y": 168, "width": 242, "height": 245}, "anchorBox": {"x": 54, "y": 377, "width": 36, "height": 36}},
            {"elementId": "n2", "hasExplicitId": True, "domIndex": 2, "tag": "g", "role": "node", "bbox": {"x": 280, "y": 377, "width": 242, "height": 209}, "anchorBox": {"x": 290, "y": 377, "width": 36, "height": 36}},
            {"elementId": "n3", "hasExplicitId": True, "domIndex": 3, "tag": "g", "role": "node", "bbox": {"x": 476, "y": 158, "width": 242, "height": 259}, "anchorBox": {"x": 522, "y": 373, "width": 44, "height": 44}},
        ]

        report = classify_measurements(self.measured(elements))

        self.assertNotIn(
            "connector_contract_incomplete",
            {item["rule"] for item in report["hardErrors"]},
        )

    def test_axis_accepts_explicit_circle_markers_without_semantic_role(self) -> None:
        elements = [
            {"elementId": "axis", "hasExplicitId": True, "domIndex": 0, "tag": "line", "role": "connector", "bbox": {"x": 560, "y": 155, "width": 0, "height": 360}, "start": {"x": 560, "y": 155}, "end": {"x": 560, "y": 515}, "routePoints": [{"x": 560, "y": 155}, {"x": 560, "y": 515}]},
            {"elementId": "node-1", "hasExplicitId": True, "domIndex": 1, "tag": "circle", "role": "", "bbox": {"x": 552, "y": 177, "width": 16, "height": 16}},
            {"elementId": "node-2", "hasExplicitId": True, "domIndex": 2, "tag": "circle", "role": "", "bbox": {"x": 552, "y": 267, "width": 16, "height": 16}},
            {"elementId": "node-3", "hasExplicitId": True, "domIndex": 3, "tag": "circle", "role": "", "bbox": {"x": 552, "y": 357, "width": 16, "height": 16}},
        ]

        report = classify_measurements(self.measured(elements))

        self.assertNotIn(
            "connector_contract_incomplete",
            {item["rule"] for item in report["hardErrors"]},
        )

    def test_callout_label_may_sit_outside_owner_when_short_leader_connects_it(self) -> None:
        elements = [
            {"elementId": "panel", "hasExplicitId": True, "domIndex": 0, "tag": "rect", "role": "card", "bbox": {"x": 460, "y": 100, "width": 780, "height": 595}},
            {"elementId": "leader", "hasExplicitId": True, "domIndex": 1, "tag": "line", "role": "", "bbox": {"x": 440, "y": 195, "width": 38, "height": 0}, "start": {"x": 440, "y": 195}, "end": {"x": 478, "y": 195}, "routePoints": [{"x": 440, "y": 195}, {"x": 478, "y": 195}]},
        ]
        texts = [
            {"elementId": "text-1", "regionId": "stage", "region": {"x": 405, "y": 175, "width": 95, "height": 20}, "bbox": {"x": 430, "y": 178, "width": 45, "height": 15}, "role": "label", "owner": "panel", "text": "阶段"},
        ]

        report = classify_measurements(self.measured(elements, texts))

        self.assertNotIn(
            "label_outside_region",
            {item["rule"] for item in report["hardErrors"]},
        )

    def test_isolated_date_label_over_node_is_moved_away_from_connector(self) -> None:
        elements = [
            {"elementId": "node", "hasExplicitId": True, "domIndex": 0, "tag": "g", "role": "node", "bbox": {"x": 44, "y": 168, "width": 242, "height": 245}, "anchorBox": {"x": 54, "y": 377, "width": 36, "height": 36}},
            {"elementId": "card", "hasExplicitId": True, "domIndex": 1, "tag": "rect", "role": "card", "bbox": {"x": 44, "y": 168, "width": 242, "height": 156}},
            {"elementId": "route", "hasExplicitId": True, "domIndex": 2, "tag": "line", "role": "connector", "connectorFrom": "node", "connectorTo": "card", "bbox": {"x": 81.5, "y": 324, "width": 34.8, "height": 55.7}, "start": {"x": 81.5, "y": 379.7}, "end": {"x": 116.3, "y": 324}, "routePoints": [{"x": 81.5, "y": 379.7}, {"x": 116.3, "y": 324}]},
        ]
        texts = [
            {"elementId": "text-1", "regionId": "date", "region": {"x": 57.9, "y": 369, "width": 28.2, "height": 16}, "bbox": {"x": 57.9, "y": 369, "width": 28.2, "height": 16}, "role": "label", "text": "1928"},
        ]

        report = classify_measurements(self.measured(elements, texts))
        issue = next(
            item for item in report["hardErrors"] if item["rule"] == "text_line_overlap"
        )
        self.assertEqual(issue["ownerId"], "node")
        svg = f'<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720"><g id="node" data-pome-visual-role="node"><circle cx="72" cy="395" r="18"/></g><rect id="card" x="44" y="168" width="242" height="156" data-pome-visual-role="card"/><line id="route" x1="81.5" y1="379.7" x2="116.3" y2="324" data-pome-visual-role="connector" data-pome-from="node" data-pome-to="card"/><text data-pome-role="label" data-pome-region-id="date" data-pome-region-x="57.9" data-pome-region-y="369" data-pome-region-width="28.2" data-pome-region-height="16" x="72" y="382" text-anchor="middle">1928</text></svg>'

        repaired, applied = deterministic_repair(svg, report)

        label = _find_region_id(ET.fromstring(repaired), "date")
        self.assertEqual(label.get("y"), "370")
        self.assertEqual(label.get("data-pome-region-y"), "357")
        self.assertIn("1928", repaired)
        self.assertTrue(
            any(item["action"] == "separate-label-from-connector" for item in applied)
        )

    def test_contract_mode_rejects_unmarked_multi_card_page(self) -> None:
        elements = [
            {"elementId": "visual-1", "hasExplicitId": False, "domIndex": 0, "tag": "rect", "role": "", "decorative": False, "bbox": {"x": 80, "y": 100, "width": 240, "height": 140}},
            {"elementId": "visual-2", "hasExplicitId": False, "domIndex": 1, "tag": "rect", "role": "", "decorative": False, "bbox": {"x": 360, "y": 100, "width": 240, "height": 140}},
        ]
        texts = [
            {"elementId": "text-1", "text": "事实一", "bbox": {"x": 100, "y": 130, "width": 60, "height": 20}},
            {"elementId": "text-2", "text": "事实二", "bbox": {"x": 380, "y": 130, "width": 60, "height": 20}},
        ]
        relaxed = classify_measurements(self.measured(elements, texts))
        strict = classify_measurements(self.measured(elements, texts), require_contract=True)
        self.assertTrue(relaxed["passed"])
        self.assertTrue(any(item["rule"] == "missing_visual_detail_metadata" for item in strict["hardErrors"]))

    def test_connector_endpoint_offset_and_text_crossing_are_detected(self) -> None:
        elements = [
            {"elementId": "n1", "hasExplicitId": True, "domIndex": 0, "tag": "circle", "role": "node", "bbox": {"x": 90, "y": 90, "width": 20, "height": 20}},
            {"elementId": "n2", "hasExplicitId": True, "domIndex": 1, "tag": "rect", "role": "card", "bbox": {"x": 300, "y": 70, "width": 120, "height": 60}},
            {"elementId": "c1", "hasExplicitId": True, "domIndex": 2, "tag": "line", "role": "connector", "connectorFrom": "n1", "connectorTo": "n2", "bbox": {"x": 115, "y": 100, "width": 175, "height": 0}, "start": {"x": 115, "y": 100}, "end": {"x": 290, "y": 100}},
        ]
        texts = [{"elementId": "text-1", "domIndex": 0, "role": "body", "text": "中文正文", "bbox": {"x": 180, "y": 90, "width": 60, "height": 20}}]
        report = classify_measurements(self.measured(elements, texts))
        rules = {item["rule"] for item in report["hardErrors"]}
        self.assertIn("connector_endpoint_not_on_node", rules)
        self.assertIn("text_line_overlap", rules)

    def test_polyline_route_segments_detect_text_crossing(self) -> None:
        elements = [
            {"elementId": "n1", "hasExplicitId": True, "domIndex": 0, "tag": "circle", "role": "node", "bbox": {"x": 90, "y": 90, "width": 20, "height": 20}},
            {"elementId": "n2", "hasExplicitId": True, "domIndex": 1, "tag": "circle", "role": "node", "bbox": {"x": 290, "y": 190, "width": 20, "height": 20}},
            {"elementId": "route", "hasExplicitId": True, "domIndex": 2, "tag": "polyline", "role": "connector", "connectorFrom": "n1", "connectorTo": "n2", "bbox": {"x": 110, "y": 100, "width": 180, "height": 100}, "start": {"x": 110, "y": 100}, "end": {"x": 290, "y": 200}, "routePoints": [{"x": 110, "y": 100}, {"x": 200, "y": 100}, {"x": 200, "y": 200}, {"x": 290, "y": 200}]},
        ]
        texts = [{"elementId": "text-1", "domIndex": 0, "role": "body", "text": "折线穿字", "bbox": {"x": 185, "y": 135, "width": 60, "height": 20}}]
        report = classify_measurements(self.measured(elements, texts))
        self.assertTrue(any(item["rule"] == "text_line_overlap" for item in report["hardErrors"]))

    def test_aligned_cards_pass_and_drift_is_detected(self) -> None:
        base = {"hasExplicitId": True, "tag": "g", "role": "card", "alignGroup": "row-1", "alignAxis": "row", "uniformSize": True}
        aligned = [dict(base, elementId="a", domIndex=0, bbox={"x": 100, "y": 120, "width": 180, "height": 100}), dict(base, elementId="b", domIndex=1, bbox={"x": 300, "y": 120, "width": 180, "height": 100})]
        self.assertFalse(any(item["rule"] == "grid_alignment_drift" for item in classify_measurements(self.measured(aligned))["hardErrors"]))
        aligned[1]["bbox"]["y"] = 125
        self.assertTrue(any(item["rule"] == "grid_alignment_drift" for item in classify_measurements(self.measured(aligned))["hardErrors"]))
        aligned[1]["bbox"]["y"] = 120
        aligned[1]["bbox"]["width"] = 190
        self.assertTrue(any(item["rule"] == "grid_size_drift" for item in classify_measurements(self.measured(aligned))["hardErrors"]))

    def test_connector_is_rerouted_around_unowned_timeline_label(self) -> None:
        svg = f'<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720"><circle id="node" cx="270" cy="390" r="9"/><rect id="card" x="88" y="440" width="364" height="72"/><line id="route" x1="270" y1="399" x2="270" y2="440" data-pome-visual-role="connector" data-pome-from="node" data-pome-to="card"/><text data-pome-role="label" data-pome-region-id="date" x="270" y="429">1927</text></svg>'
        report = {
            "visualElements": [
                {"elementId": "route", "tag": "line", "role": "connector", "connectorFrom": "node", "connectorTo": "card", "bbox": {"x": 270, "y": 399, "width": 0, "height": 41}, "start": {"x": 270, "y": 399}, "end": {"x": 270, "y": 440}},
            ],
            "textElements": [
                {"elementId": "text-1", "regionId": "date", "role": "label", "bbox": {"x": 255, "y": 416, "width": 30, "height": 16}},
            ],
            "hardErrors": [
                {"rule": "text_line_overlap", "elementId": "text-1", "role": "label", "regionId": "date", "actualBounds": {"x": 255, "y": 416, "width": 30, "height": 16}, "collision": {"elementId": "route", "start": {"x": 270, "y": 399}, "end": {"x": 270, "y": 440}}},
            ],
        }

        repaired, applied = deterministic_repair(svg, report)

        route = _find_id(ET.fromstring(repaired), "route")
        self.assertEqual(_local_name(route.tag), "polyline")
        self.assertIn("1927", repaired)
        self.assertTrue(
            any(item["action"] == "reroute-connector-around-label" for item in applied)
        )

    def test_visible_text_is_not_changed_by_connector_repair(self) -> None:
        svg = f'<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720"><circle id="n1" cx="100" cy="100" r="10"/><rect id="n2" x="300" y="70" width="120" height="60"/><line id="c1" x1="115" y1="100" x2="290" y2="100" data-pome-visual-role="connector" data-pome-from="n1" data-pome-to="n2"/><text x="20" y="30">事实文字</text></svg>'
        report = {"visualElements": [{"elementId": "c1", "bbox": {}, "tag": "line"}], "hardErrors": [{"rule": "connector_endpoint_not_on_node", "elementId": "c1", "endpoint": "start", "targetId": "n1", "expectedPoint": {"x": 110, "y": 100}, "offsetDistance": 5}]}
        repaired, applied = deterministic_repair(svg, report)
        self.assertIn("事实文字", repaired)
        self.assertEqual(len(applied), 1)

    def test_unrepairable_overlap_requests_page_relayout(self) -> None:
        svg = f'<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720"><rect id="a" x="10" y="10" width="100" height="100"/></svg>'
        report = {"visualElements": [], "hardErrors": [{"rule": "card_card_overlap", "elementId": "a"}]}
        repaired, applied = deterministic_repair(svg, report)
        self.assertEqual(svg, repaired)
        self.assertEqual(applied, [])

    def test_small_same_row_card_overlap_moves_the_wrapping_group_only(self) -> None:
        svg = f'<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720"><g id="a"><rect width="100" height="80"/><text>甲</text></g><g id="b"><rect width="100" height="80"/><text>乙</text></g></svg>'
        report = {
            "visualElements": [
                {"elementId": "a", "tag": "g", "alignGroup": "row", "alignAxis": "row", "bbox": {"x": 100, "y": 100, "width": 100, "height": 80}},
                {"elementId": "b", "tag": "g", "alignGroup": "row", "alignAxis": "row", "bbox": {"x": 196, "y": 100, "width": 100, "height": 80}},
            ],
            "hardErrors": [{"rule": "card_card_overlap", "elementId": "a", "collision": {"elementId": "b", "intersection": {"x": 196, "y": 100, "width": 4, "height": 80}}}],
        }
        repaired, applied = deterministic_repair(svg, report)
        self.assertIn('id="b" transform="translate(12 0)"', repaired)
        self.assertIn("甲", repaired)
        self.assertIn("乙", repaired)
        self.assertTrue(any(item["action"] == "separate-overlapping-card-group" for item in applied))

    def test_small_ungrouped_horizontal_card_overlap_uses_safe_local_space(self) -> None:
        svg = f'<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720"><g id="a" data-pome-visual-role="card"><rect x="685" y="240" width="190" height="220"/><text>甲</text></g><g id="b" data-pome-visual-role="card"><rect x="870" y="300" width="180" height="170"/><text>乙</text></g></svg>'
        report = {
            "visualElements": [
                {"elementId": "a", "tag": "g", "role": "card", "bbox": {"x": 685, "y": 240, "width": 190, "height": 220}},
                {"elementId": "b", "tag": "g", "role": "card", "bbox": {"x": 870, "y": 300, "width": 180, "height": 170}},
            ],
            "hardErrors": [{"rule": "card_card_overlap", "elementId": "a", "collision": {"elementId": "b", "intersection": {"x": 870, "y": 300, "width": 5, "height": 160}}}],
        }

        repaired, applied = deterministic_repair(svg, report)

        self.assertIn('id="b" data-pome-visual-role="card" transform="translate(13 0)"', repaired)
        self.assertIn("甲", repaired)
        self.assertIn("乙", repaired)
        self.assertTrue(
            any(
                item["action"] == "separate-overlapping-card-group"
                for item in applied
            )
        )

    def test_small_rect_card_overlap_shrinks_only_when_owned_text_still_fits(self) -> None:
        svg = f'''<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720">
        <rect id="a" x="100" y="100" width="220" height="100" data-pome-visual-role="card"/>
        <rect id="b" x="300" y="100" width="220" height="100" data-pome-visual-role="card"/>
        <text data-pome-region-id="a-body" data-pome-owner="a" x="116" y="140">事实文字</text>
        </svg>'''
        report = {
            "visualElements": [
                {"elementId": "a", "tag": "rect", "role": "card", "bbox": {"x": 100, "y": 100, "width": 220, "height": 100}},
                {"elementId": "b", "tag": "rect", "role": "card", "bbox": {"x": 300, "y": 100, "width": 220, "height": 100}},
            ],
            "textElements": [
                {"elementId": "text-1", "owner": "a", "bbox": {"x": 116, "y": 124, "width": 80, "height": 20}},
            ],
            "hardErrors": [
                {"rule": "card_card_overlap", "elementId": "a", "collision": {"elementId": "b", "intersection": {"x": 300, "y": 100, "width": 20, "height": 100}}},
            ],
        }

        repaired, applied = deterministic_repair(svg, report)

        root = ET.fromstring(repaired)
        self.assertEqual(_find_id(root, "a").get("width"), "192")
        self.assertIn("事实文字", repaired)
        self.assertTrue(any(item["action"] == "shrink-overlapping-card-right-edge" for item in applied))

    def test_small_vertical_overlap_moves_unconnected_lower_card_and_its_text(self) -> None:
        svg = f'''<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720">
        <rect id="upper" x="330" y="530" width="156" height="64" data-pome-visual-role="card"/>
        <rect id="lower" x="48" y="590" width="480" height="82" data-pome-visual-role="card"/>
        <text data-pome-region-id="lower-body" data-pome-region-y="590" data-pome-owner="lower" x="70"><tspan x="70" y="616">可见事实</tspan><tspan x="70" dy="22">保持不变</tspan></text>
        </svg>'''
        report = {
            "visualElements": [
                {"elementId": "upper", "tag": "rect", "role": "card", "bbox": {"x": 330, "y": 530, "width": 156, "height": 64}},
                {"elementId": "lower", "tag": "rect", "role": "card", "bbox": {"x": 48, "y": 590, "width": 480, "height": 82}},
            ],
            "textElements": [
                {"elementId": "text-1", "owner": "lower", "regionId": "lower-body", "bbox": {"x": 70, "y": 600, "width": 80, "height": 42}},
            ],
            "hardErrors": [
                {"rule": "card_card_overlap", "elementId": "upper", "collision": {"elementId": "lower", "intersection": {"x": 330, "y": 590, "width": 156, "height": 4}}},
            ],
        }

        repaired, applied = deterministic_repair(svg, report)

        root = ET.fromstring(repaired)
        self.assertEqual(_find_id(root, "lower").get("y"), "602")
        text = _find_region_id(root, "lower-body")
        self.assertEqual(text.get("data-pome-region-y"), "602")
        self.assertEqual(text.find(f"{{{SVG_NS}}}tspan").get("y"), "628")
        self.assertEqual("".join(text.itertext()), "可见事实保持不变")
        self.assertTrue(
            any(
                item["action"] == "separate-vertically-overlapping-card"
                for item in applied
            )
        )


class VisualDetailBrowserTests(unittest.TestCase):
    def test_vertical_connector_is_snapped_then_routed_around_owned_label(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-visual-details-route-")
        path = Path(directory.name) / "connector-through-label.svg"
        path.write_text(
            f'''<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff" data-pome-decorative="true"/>
<circle id="node" cx="200" cy="200" r="10" fill="#2563eb" data-pome-visual-role="node"/>
<rect id="card" x="150" y="300" width="100" height="80" fill="#eef2ff" data-pome-visual-role="card"/>
<line id="route" x1="200" y1="220" x2="200" y2="290" stroke="#2563eb" stroke-width="2"
 data-pome-visual-role="connector" data-pome-from="node" data-pome-to="card"/>
<text text-anchor="middle" data-pome-role="label" data-pome-region-id="node-label"
 data-pome-region-x="150" data-pome-region-y="230" data-pome-region-width="100"
 data-pome-region-height="24" data-pome-min-font-size="12" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-line-height="18" data-pome-safe-padding="4"
 data-pome-owner="node" x="200" y="247" font-family="Microsoft YaHei" font-size="14">节点事实</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before = run(path, auto_fix=False)
            self.assertFalse(before["passed"])
            after = run(path, auto_fix=True)
            self.assertTrue(after["passed"], after)
            self.assertEqual(after["visibleTexts"], ["节点事实"])
            updated = path.read_text(encoding="utf-8")
            self.assertIn("<polyline", updated)
            self.assertIn('data-pome-from="node"', updated)
            self.assertTrue(
                any(
                    item["action"] == "reroute-connector-around-label"
                    for item in after["autoFixApplied"]
                )
            )
        finally:
            directory.cleanup()

    def test_id_assignment_is_committed_when_an_existing_contract_error_remains(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-visual-details-id-")
        path = Path(directory.name) / "connector-without-contract.svg"
        path.write_text(
            f'''<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff" data-pome-decorative="true"/>
<line x1="100" y1="100" x2="300" y2="100" stroke="#2563eb" stroke-width="2" data-pome-visual-role="connector"/>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, auto_fix=True)
            self.assertFalse(report["passed"])
            self.assertNotIn("autoFixRejected", report)
            self.assertEqual([item["rule"] for item in report["hardErrors"]], ["connector_contract_incomplete"])
            self.assertIn('id="pome-auto-connector-2"', path.read_text(encoding="utf-8"))
            self.assertTrue(any(item["action"] == "assign-structural-element-id" for item in report["autoFixApplied"]))
        finally:
            directory.cleanup()

    def test_connector_snap_is_transactional_and_keeps_visible_text(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-visual-details-")
        path = Path(directory.name) / "connector.svg"
        path.write_text(
            f'''<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff" data-pome-decorative="true"/>
<circle id="n1" cx="100" cy="100" r="10" fill="#2563eb" data-pome-visual-role="node"/>
<rect id="n2" x="300" y="70" width="120" height="60" fill="#eef2ff" data-pome-visual-role="card"/>
<line id="c1" x1="115" y1="100" x2="290" y2="100" stroke="#2563eb" stroke-width="2"
 data-pome-visual-role="connector" data-pome-from="n1" data-pome-to="n2"/>
<text id="fact" x="20" y="30" font-family="Microsoft YaHei" font-size="16">事实文字</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before = run(path, auto_fix=False)
            self.assertFalse(before["passed"])
            after = run(path, auto_fix=True)
            self.assertTrue(after["passed"], after)
            self.assertEqual(after["visibleTexts"], ["事实文字"])
            self.assertEqual(len(after["autoFixApplied"]), 2)
            updated = path.read_text(encoding="utf-8")
            self.assertIn('x1="110"', updated)
            self.assertIn('x2="300"', updated)
        finally:
            directory.cleanup()

    def test_connector_snap_that_creates_text_collision_is_rejected(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-visual-details-rollback-")
        path = Path(directory.name) / "connector-collision.svg"
        original = f'''<svg xmlns="{SVG_NS}" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff" data-pome-decorative="true"/>
<circle id="n1" cx="100" cy="100" r="10" fill="#2563eb" data-pome-visual-role="node"/>
<rect id="n2" x="300" y="70" width="120" height="60" fill="#eef2ff" data-pome-visual-role="card"/>
<line id="c1" x1="115" y1="100" x2="290" y2="100" stroke="#2563eb" stroke-width="2"
 data-pome-visual-role="connector" data-pome-from="n1" data-pome-to="n2"/>
<text id="collision" x="292" y="104" font-family="Microsoft YaHei" font-size="16">冲突</text>
</svg>'''
        path.write_text(original, encoding="utf-8")
        try:
            report = run(path, auto_fix=True)
            self.assertFalse(report["passed"])
            self.assertIn("autoFixRejected", report)
            self.assertIn('x2="290"', path.read_text(encoding="utf-8"))
            self.assertIn("冲突", path.read_text(encoding="utf-8"))
        finally:
            directory.cleanup()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--svg", type=Path)
    parser.add_argument("--auto-fix", action="store_true")
    parser.add_argument("--require-contract", action="store_true")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.TestSuite(
            [
                unittest.defaultTestLoader.loadTestsFromTestCase(VisualDetailClassificationTests),
                unittest.defaultTestLoader.loadTestsFromTestCase(VisualDetailBrowserTests),
            ]
        )
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        return 0 if result.wasSuccessful() else 1
    if args.svg is None:
        parser.error("--svg is required unless --self-test is used")
    try:
        report = run(args.svg, auto_fix=args.auto_fix, require_contract=args.require_contract)
        serialized = json.dumps(report, ensure_ascii=False)
        if args.report:
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
