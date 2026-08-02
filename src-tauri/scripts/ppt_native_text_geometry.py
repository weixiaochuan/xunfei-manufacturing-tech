#!/usr/bin/env python3
"""Measure native SVG text with Chromium and enforce declared text regions.

This helper belongs to Pomegranate's strict native pipeline.  It intentionally
uses the browser's SVG engine (`getBBox()` plus the element CTM) instead of a
character-count approximation, so anchors, tspans, transforms, and the actual
installed fonts are reflected in the result.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.request
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from websockets.sync.client import connect


CANVAS = {"x": 0.0, "y": 0.0, "width": 1280.0, "height": 720.0}
DEFAULT_SAFE_PADDING = {
    "title": 10.0,
    "subtitle": 8.0,
    "body": 10.0,
    "metric": 8.0,
    "unit": 6.0,
    "caption": 6.0,
    "label": 6.0,
    "footer": 4.0,
}


def _number(value: Any, default: float | None = None) -> float | None:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return default
    if number != number or number in (float("inf"), float("-inf")):
        return default
    return number


def _bbox(raw: dict[str, Any]) -> dict[str, float]:
    return {
        "x": float(raw["x"]),
        "y": float(raw["y"]),
        "width": float(raw["width"]),
        "height": float(raw["height"]),
    }


def _right(box: dict[str, float]) -> float:
    return box["x"] + box["width"]


def _bottom(box: dict[str, float]) -> float:
    return box["y"] + box["height"]


def _outside(actual: dict[str, float], allowed: dict[str, float], tolerance: float = 0.75) -> dict[str, float]:
    return {
        "left": max(0.0, allowed["x"] - actual["x"] - tolerance),
        "top": max(0.0, allowed["y"] - actual["y"] - tolerance),
        "right": max(0.0, _right(actual) - _right(allowed) - tolerance),
        "bottom": max(0.0, _bottom(actual) - _bottom(allowed) - tolerance),
    }


def _has_overflow(amounts: dict[str, float]) -> bool:
    return any(value > 0.0 for value in amounts.values())


def _inner(box: dict[str, float], padding: float) -> dict[str, float]:
    return {
        "x": box["x"] + padding,
        "y": box["y"] + padding,
        "width": max(0.0, box["width"] - padding * 2.0),
        "height": max(0.0, box["height"] - padding * 2.0),
    }


def _intersection(left: dict[str, float], right: dict[str, float]) -> dict[str, float] | None:
    x1 = max(left["x"], right["x"])
    y1 = max(left["y"], right["y"])
    x2 = min(_right(left), _right(right))
    y2 = min(_bottom(left), _bottom(right))
    if x2 <= x1 or y2 <= y1:
        return None
    return {"x": x1, "y": y1, "width": x2 - x1, "height": y2 - y1}


def _issue(
    severity: str,
    rule: str,
    block: dict[str, Any],
    message: str,
    *,
    allowed: dict[str, float] | None = None,
    overflow: dict[str, float] | None = None,
    collision: dict[str, Any] | None = None,
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "severity": severity,
        "rule": rule,
        "elementId": block.get("elementId") or None,
        "domIndex": block.get("domIndex"),
        "regionId": block.get("regionId") or None,
        "role": block.get("role") or None,
        "text": block.get("text", ""),
        "actualBounds": block.get("bbox"),
        "message": message,
    }
    if allowed is not None:
        result["allowedBounds"] = allowed
    if overflow is not None:
        result["overflow"] = overflow
    if collision is not None:
        result["collision"] = collision
    return result


def classify_measurements(
    measured: dict[str, Any], require_markers: bool = True
) -> dict[str, Any]:
    """Classify exact browser measurements without estimating text width."""
    hard: list[dict[str, Any]] = []
    warnings: list[dict[str, Any]] = []
    texts = measured.get("texts", [])

    for block in texts:
        actual = _bbox(block["bbox"])
        block["bbox"] = actual
        canvas_overflow = _outside(actual, CANVAS)
        if _has_overflow(canvas_overflow):
            hard.append(
                _issue(
                    "hard",
                    "text_outside_canvas",
                    block,
                    "文字实际边界超出 1280×720 画布",
                    allowed=CANVAS,
                    overflow=canvas_overflow,
                )
            )

        if block.get("missingFirstLineBaseline"):
            hard.append(
                _issue(
                    "hard",
                    "missing_first_line_baseline",
                    block,
                    "多行文字首行缺少绝对 y 基线，浏览器会从默认基线 0 开始排版",
                )
            )

        marker_fields = ("role", "regionId", "region")
        if "contractMissingFields" in block:
            missing = list(block.get("contractMissingFields") or [])
        else:
            missing = [name for name in marker_fields if not block.get(name)]
        if missing:
            if require_markers:
                hard.append(
                    _issue(
                        "hard",
                        "missing_text_region_metadata",
                        block,
                        f"文字块缺少原生几何语义标记: {', '.join(missing)}",
                    )
                )
            continue

        region = _bbox(block["region"])
        region_canvas_overflow = _outside(region, CANVAS)
        if _has_overflow(region_canvas_overflow):
            hard.append(
                _issue(
                    "hard",
                    "text_region_outside_canvas",
                    block,
                    "声明的文字区域超出 1280×720 画布",
                    allowed=CANVAS,
                    overflow=region_canvas_overflow,
                )
            )
        region_overflow = _outside(actual, region)
        if _has_overflow(region_overflow):
            hard.append(
                _issue(
                    "hard",
                    "text_outside_declared_region",
                    block,
                    "文字实际边界超出声明的 layout region",
                    allowed=region,
                    overflow=region_overflow,
                )
            )
        else:
            padding = _number(block.get("safePadding"))
            if padding is None:
                padding = DEFAULT_SAFE_PADDING.get(block.get("role", ""), 8.0)
            safe_box = _inner(region, padding)
            padding_overflow = _outside(actual, safe_box, tolerance=0.25)
            if _has_overflow(padding_overflow):
                warnings.append(
                    _issue(
                        "warning",
                        "text_safe_padding_tight",
                        block,
                        "文字未出框，但未满足声明的安全内边距",
                        allowed=safe_box,
                        overflow=padding_overflow,
                    )
                )

        max_lines = int(_number(block.get("maxLines"), 0.0) or 0)
        line_count = int(block.get("lineCount") or 1)
        if max_lines > 0 and line_count > max_lines:
            hard.append(
                _issue(
                    "hard",
                    "text_exceeds_max_lines",
                    block,
                    f"实际 {line_count} 行，超过允许的 {max_lines} 行",
                    allowed=region,
                )
            )

    for index, left in enumerate(texts):
        if left.get("allowOverlap"):
            continue
        for right in texts[index + 1 :]:
            if right.get("allowOverlap"):
                continue
            if left.get("collisionScope") and right.get("collisionScope"):
                if left["collisionScope"] != right["collisionScope"]:
                    continue
            overlap = _intersection(_bbox(left["bbox"]), _bbox(right["bbox"]))
            if overlap is None or overlap["width"] <= 1.0 or overlap["height"] <= 1.0:
                continue
            min_height = min(float(left["bbox"]["height"]), float(right["bbox"]["height"]))
            is_hard = overlap["width"] >= 3.0 and overlap["height"] >= max(3.0, min_height * 0.12)
            target = {
                "type": "text",
                "elementId": right.get("elementId") or None,
                "domIndex": right.get("domIndex"),
                "regionId": right.get("regionId") or None,
                "text": right.get("text", ""),
                "bounds": right.get("bbox"),
                "intersection": overlap,
            }
            item = _issue(
                "hard" if is_hard else "warning",
                "text_text_overlap" if is_hard else "text_text_spacing_tight",
                left,
                "文字块之间发生明显覆盖" if is_hard else "文字块之间的垂直/水平间距过小",
                collision=target,
            )
            (hard if is_hard else warnings).append(item)

    obstacles = measured.get("obstacles", [])
    for block in texts:
        if block.get("allowOverlap"):
            continue
        for obstacle in obstacles:
            # A semantic obstacle is often a complete visual node whose own
            # labels are descendants of that group.  Those labels are content
            # of the node, not collisions with it.  Only sibling/independent
            # obstacles participate in obstacle collision checks.
            if block.get("domIndex") in obstacle.get("containsTextDomIndexes", []):
                continue
            if obstacle.get("regionId") and block.get("regionId"):
                if obstacle["regionId"] != block["regionId"]:
                    continue
                # Native SVGs sometimes mark a whole card background group as
                # an obstacle and place the card body immediately after that
                # group.  A same-region obstacle that fully contains the
                # declared text region is the region's visual container, not
                # an icon obscuring its text.  Smaller obstacles still collide.
                if block.get("region"):
                    declared_region = _bbox(block["region"])
                    obstacle_box = _bbox(obstacle["bbox"])
                    if not _has_overflow(_outside(declared_region, obstacle_box)):
                        continue
            overlap = _intersection(_bbox(block["bbox"]), _bbox(obstacle["bbox"]))
            if overlap is None or overlap["width"] <= 1.0 or overlap["height"] <= 1.0:
                continue
            hard.append(
                _issue(
                    "hard",
                    "text_obstacle_overlap",
                    block,
                    "文字与声明为 obstacle 的图标或图形发生覆盖",
                    collision={
                        "type": "obstacle",
                        "elementId": obstacle.get("elementId") or None,
                        "regionId": obstacle.get("regionId") or None,
                        "bounds": obstacle.get("bbox"),
                        "intersection": overlap,
                    },
                )
            )

    return {
        "schemaVersion": 1,
        "canvas": CANVAS,
        "regionCoordinateSpace": "canvas",
        "svgPath": measured.get("svgPath"),
        "passed": not hard,
        "hardErrors": hard,
        "warnings": warnings,
        "textBlocks": texts,
        "measurements": {"textCount": len(texts), "obstacleCount": len(obstacles)},
    }


MEASURE_SCRIPT = r"""
(async () => {
  await document.fonts.ready;
  const root = document.documentElement;
  const rootInverse = root.getScreenCTM().inverse();
  function rootBox(element) {
    const box = element.getBBox();
    const matrix = rootInverse.multiply(element.getScreenCTM());
    const points = [
      new DOMPoint(box.x, box.y),
      new DOMPoint(box.x + box.width, box.y),
      new DOMPoint(box.x, box.y + box.height),
      new DOMPoint(box.x + box.width, box.y + box.height),
    ].map((point) => point.matrixTransform(matrix));
    const xs = points.map((point) => point.x);
    const ys = points.map((point) => point.y);
    const x = Math.min(...xs);
    const y = Math.min(...ys);
    return {x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y};
  }
  function numberAttr(element, name) {
    const raw = element.getAttribute(name);
    if (raw === null || raw.trim() === '') return null;
    const value = Number(raw);
    return Number.isFinite(value) ? value : null;
  }
  function region(element) {
    const x = numberAttr(element, 'data-pome-region-x');
    const y = numberAttr(element, 'data-pome-region-y');
    const width = numberAttr(element, 'data-pome-region-width');
    const height = numberAttr(element, 'data-pome-region-height');
    if ([x, y, width, height].some((value) => value === null)) return null;
    return {x, y, width, height};
  }
  function boolAttr(element, name) {
    return (element.getAttribute(name) || '').toLowerCase() === 'true';
  }
  function rawAttr(element, name) {
    const value = element.getAttribute(name);
    return value === null ? null : value;
  }
  function localToCanvas(element) {
    const matrix = rootInverse.multiply(element.getScreenCTM());
    return {
      a: matrix.a, b: matrix.b, c: matrix.c,
      d: matrix.d, e: matrix.e, f: matrix.f,
    };
  }
  const textElements = [...root.querySelectorAll('text')].filter((element) => {
    const style = getComputedStyle(element);
    return style.display !== 'none' && style.visibility !== 'hidden' && Number(style.opacity) !== 0;
  });
  const texts = textElements.map((element, index) => {
    const tspans = [...element.querySelectorAll(':scope > tspan')];
    let lineCount = 1;
    if (tspans.length) {
      lineCount = tspans.reduce((count, tspan, tspanIndex) => {
        const startsLine = tspanIndex === 0 || tspan.hasAttribute('x') || tspan.hasAttribute('y') || tspan.hasAttribute('dy');
        return count + (startsLine ? 1 : 0);
      }, 0);
      lineCount = Math.max(1, lineCount);
    }
    const style = getComputedStyle(element);
    const firstTspan = tspans[0] || null;
    const missingFirstLineBaseline = Boolean(
      firstTspan && !element.hasAttribute('y') && !firstTspan.hasAttribute('y')
    );
    const contractMissingFields = [];
    if (!element.getAttribute('data-pome-role')) contractMissingFields.push('role');
    if (!element.getAttribute('data-pome-region-id')) contractMissingFields.push('regionId');
    if (!region(element)) contractMissingFields.push('region');
    if (!element.hasAttribute('data-pome-min-font-size')) contractMissingFields.push('minFontSize');
    if (!element.hasAttribute('data-pome-wrap')) contractMissingFields.push('wrap');
    if (!element.hasAttribute('data-pome-max-lines')) contractMissingFields.push('maxLines');
    if (!element.hasAttribute('data-pome-line-height')) contractMissingFields.push('lineHeight');
    if (!element.hasAttribute('text-anchor')) contractMissingFields.push('textAnchor');
    return {
      elementId: element.id || `text-${index + 1}`,
      domIndex: index,
      text: (element.textContent || '').replace(/\s+/g, ' ').trim(),
      bbox: rootBox(element),
      role: element.getAttribute('data-pome-role'),
      regionId: element.getAttribute('data-pome-region-id'),
      region: region(element),
      minFontSize: numberAttr(element, 'data-pome-min-font-size'),
      safePadding: numberAttr(element, 'data-pome-safe-padding'),
      maxLines: numberAttr(element, 'data-pome-max-lines'),
      wrap: boolAttr(element, 'data-pome-wrap'),
      allowOverlap: boolAttr(element, 'data-pome-allow-overlap'),
      collisionScope: element.getAttribute('data-pome-collision-scope'),
      fontSize: Number.parseFloat(style.fontSize),
      fontWeight: style.fontWeight,
      textAnchor: style.textAnchor || element.getAttribute('text-anchor') || 'start',
      transform: element.getAttribute('transform'),
      localToCanvas: localToCanvas(element),
      rawPosition: {
        x: rawAttr(element, 'x'),
        y: rawAttr(element, 'y'),
        dx: rawAttr(element, 'dx'),
        dy: rawAttr(element, 'dy'),
      },
      lineHeight: numberAttr(element, 'data-pome-line-height'),
      missingFirstLineBaseline,
      contractMissingFields,
      tspans: tspans.map((tspan, tspanIndex) => {
        const tspanStyle = getComputedStyle(tspan);
        return {
          index: tspanIndex,
          text: (tspan.textContent || '').replace(/\s+/g, ' ').trim(),
          x: rawAttr(tspan, 'x'),
          y: rawAttr(tspan, 'y'),
          dx: rawAttr(tspan, 'dx'),
          dy: rawAttr(tspan, 'dy'),
          fontSize: Number.parseFloat(tspanStyle.fontSize),
          fontWeight: tspanStyle.fontWeight,
          fill: tspanStyle.fill,
          bbox: rootBox(tspan),
        };
      }),
      lineCount,
    };
  });
  const obstacles = [...root.querySelectorAll('[data-pome-obstacle="true"]')].map((element, index) => ({
    elementId: element.id || `obstacle-${index + 1}`,
    regionId: element.getAttribute('data-pome-region-id'),
    bbox: rootBox(element),
    containsTextDomIndexes: textElements.reduce((indexes, textElement, textIndex) => {
      if (element.contains(textElement)) indexes.push(textIndex);
      return indexes;
    }, []),
  }));
  return {texts, obstacles};
})()
"""


CONTRACT_NORMALIZE_SCRIPT = r"""
(async (targetIndexes) => {
  await document.fonts.ready;
  const root = document.documentElement;
  const rootInverse = root.getScreenCTM().inverse();
  const elements = [...root.querySelectorAll('text')].filter((element) => {
    const style = getComputedStyle(element);
    return style.display !== 'none' && style.visibility !== 'hidden' && Number(style.opacity) !== 0;
  });
  function numberAttr(element, name, fallback = null) {
    const raw = element.getAttribute(name);
    if (raw === null || raw.trim() === '') return fallback;
    const value = Number(raw);
    return Number.isFinite(value) ? value : fallback;
  }
  function rootMatrix(element) {
    return rootInverse.multiply(element.getScreenCTM());
  }
  function rootBox(element) {
    const box = element.getBBox();
    const matrix = rootMatrix(element);
    const points = [
      new DOMPoint(box.x, box.y),
      new DOMPoint(box.x + box.width, box.y),
      new DOMPoint(box.x, box.y + box.height),
      new DOMPoint(box.x + box.width, box.y + box.height),
    ].map((point) => point.matrixTransform(matrix));
    const xs = points.map((point) => point.x);
    const ys = points.map((point) => point.y);
    return {
      x: Math.min(...xs), y: Math.min(...ys),
      width: Math.max(...xs) - Math.min(...xs),
      height: Math.max(...ys) - Math.min(...ys),
    };
  }
  function declaredRegion(element) {
    const region = {
      x: numberAttr(element, 'data-pome-region-x'),
      y: numberAttr(element, 'data-pome-region-y'),
      width: numberAttr(element, 'data-pome-region-width'),
      height: numberAttr(element, 'data-pome-region-height'),
    };
    return Object.values(region).every((value) => Number.isFinite(value)) ? region : null;
  }
  function inferredLineHeight(element, tspans) {
    for (let index = 1; index < tspans.length; index += 1) {
      const dy = numberAttr(tspans[index], 'dy');
      if (Number.isFinite(dy) && Math.abs(dy) > 0.01) return Math.abs(dy);
      const currentY = numberAttr(tspans[index], 'y');
      const previousY = numberAttr(tspans[index - 1], 'y');
      if (Number.isFinite(currentY) && Number.isFinite(previousY) && Math.abs(currentY - previousY) > 0.01) {
        return Math.abs(currentY - previousY);
      }
    }
    const sizes = [element, ...tspans]
      .map((node) => Number.parseFloat(getComputedStyle(node).fontSize))
      .filter((value) => Number.isFinite(value));
    return Math.max(1, ...(sizes.length ? sizes : [16])) * 1.2;
  }
  const applied = [];
  elementLoop: for (const index of targetIndexes) {
    const element = elements[index];
    if (!element) continue;
    const text = (element.textContent || '').replace(/\s+/g, ' ').trim();
    const tspans = [...element.querySelectorAll(':scope > tspan')];

    if (!element.hasAttribute('text-anchor')) {
      const computedAnchor = (getComputedStyle(element).textAnchor || 'start').toLowerCase();
      const anchor = ['start', 'middle', 'end'].includes(computedAnchor) ? computedAnchor : 'start';
      element.setAttribute('text-anchor', anchor);
      applied.push({domIndex: index, text, action: 'declare-text-anchor', textAnchor: anchor});
    }
    if (!element.hasAttribute('data-pome-line-height')) {
      const lineHeight = inferredLineHeight(element, tspans);
      element.setAttribute('data-pome-line-height', String(Number(lineHeight.toFixed(3))));
      applied.push({domIndex: index, text, action: 'declare-line-height', lineHeight});
    }

    const first = tspans[0];
    const region = declaredRegion(element);
    if (!first || !region || element.hasAttribute('y') || first.hasAttribute('y')) continue;

    // A dy-only first tspan is relative to SVG's implicit baseline 0. Establish
    // one absolute baseline for the whole block before collision decisions so
    // several malformed blocks at y=0 cannot block one another by processing order.
    const matrix = rootMatrix(element);
    const axisAligned = Math.abs(matrix.b) <= 1e-6 && Math.abs(matrix.c) <= 1e-6 && Math.abs(matrix.d) > 1e-6;
    if (!axisAligned) continue;
    const before = rootBox(element);
    const targetTop = region.y + Math.max(0, (region.height - before.height) / 2);
    const deltaRootY = targetTop - before.y;
    const firstDy = numberAttr(first, 'dy', 0);
    const localBaselineY = firstDy + deltaRootY / matrix.d;
    first.setAttribute('y', String(Number(localBaselineY.toFixed(3))));
    first.removeAttribute('dy');
    const after = rootBox(element);
    applied.push({
      domIndex: index,
      text,
      action: 'establish-first-line-baseline',
      beforeBounds: before,
      afterBounds: after,
      region,
    });
  }
  return {applied, svg: new XMLSerializer().serializeToString(root)};
})
"""


SAFE_FIX_SCRIPT = r"""
(async (targetIndexes) => {
  await document.fonts.ready;
  const root = document.documentElement;
  const rootInverse = root.getScreenCTM().inverse();
  const elements = [...root.querySelectorAll('text')].filter((element) => {
    const style = getComputedStyle(element);
    return style.display !== 'none' && style.visibility !== 'hidden' && Number(style.opacity) !== 0;
  });
  function rootBox(element) {
    const box = element.getBBox();
    const matrix = rootInverse.multiply(element.getScreenCTM());
    const points = [
      new DOMPoint(box.x, box.y),
      new DOMPoint(box.x + box.width, box.y),
      new DOMPoint(box.x, box.y + box.height),
      new DOMPoint(box.x + box.width, box.y + box.height),
    ].map((point) => point.matrixTransform(matrix));
    const xs = points.map((point) => point.x);
    const ys = points.map((point) => point.y);
    const x = Math.min(...xs);
    const y = Math.min(...ys);
    return {x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y};
  }
  function numberAttr(element, name, fallback) {
    const raw = element.getAttribute(name);
    if (raw === null || raw.trim() === '') return fallback;
    const value = Number(raw);
    return Number.isFinite(value) ? value : fallback;
  }
  function fits(box, allowed) {
    const tolerance = 0.75;
    return box.x >= allowed.x - tolerance && box.y >= allowed.y - tolerance &&
      box.x + box.width <= allowed.x + allowed.width + tolerance &&
      box.y + box.height <= allowed.y + allowed.height + tolerance;
  }
  function expandAllowedToBox(allowed, box, maxExpansion = 12) {
    const left = Math.max(0, allowed.x - box.x);
    const top = Math.max(0, allowed.y - box.y);
    const right = Math.max(0, box.x + box.width - allowed.x - allowed.width);
    const bottom = Math.max(0, box.y + box.height - allowed.y - allowed.height);
    const expansion = left + top + right + bottom;
    if (expansion <= 0.01) return null;
    const region = {
      x: allowed.x - left,
      y: allowed.y - top,
      width: allowed.width + left + right,
      height: allowed.height + top + bottom,
    };
    if (
      expansion > maxExpansion + 0.01 || region.x < 0 || region.y < 0 ||
      region.x + region.width > 1280 || region.y + region.height > 720
    ) return null;
    return {region, expansion};
  }
  function intersects(first, second) {
    return Math.min(first.x + first.width, second.x + second.width) - Math.max(first.x, second.x) > 0.75 &&
      Math.min(first.y + first.height, second.y + second.height) - Math.max(first.y, second.y) > 0.75;
  }
  function collidesWithOtherText(box, index) {
    const current = elements[index];
    if (current && current.getAttribute('data-pome-allow-overlap') === 'true') return false;
    return elements.some((other, otherIndex) =>
      otherIndex !== index &&
      other.getAttribute('data-pome-allow-overlap') !== 'true' &&
      !(
        current && current.getAttribute('data-pome-collision-scope') &&
        other.getAttribute('data-pome-collision-scope') &&
        current.getAttribute('data-pome-collision-scope') !== other.getAttribute('data-pome-collision-scope')
      ) &&
      intersects(box, rootBox(other))
    );
  }
  function collidesWithObstacle(box, index) {
    const current = elements[index];
    const currentRegion = current && current.getAttribute('data-pome-region-id');
    const declaredRegion = current && {
      x: numberAttr(current, 'data-pome-region-x', NaN),
      y: numberAttr(current, 'data-pome-region-y', NaN),
      width: numberAttr(current, 'data-pome-region-width', NaN),
      height: numberAttr(current, 'data-pome-region-height', NaN),
    };
    const hasDeclaredRegion = declaredRegion &&
      Object.values(declaredRegion).every((value) => Number.isFinite(value));
    return [...root.querySelectorAll('[data-pome-obstacle="true"]')]
      .some((obstacle) => {
        // A group's own text is part of that visual node.  Treating the
        // ancestor group as an obstacle for its descendants creates a false
        // collision for every label in a timeline/process node.
        if (current && obstacle.contains(current)) return false;
        const obstacleRegion = obstacle.getAttribute('data-pome-region-id');
        if (currentRegion && obstacleRegion && currentRegion !== obstacleRegion) return false;
        const obstacleBox = rootBox(obstacle);
        if (
          currentRegion && obstacleRegion && currentRegion === obstacleRegion &&
          hasDeclaredRegion && fits(declaredRegion, obstacleBox)
        ) return false;
        return intersects(box, obstacleBox);
      });
  }
  function setLineX(element, value) {
    element.setAttribute('x', String(value));
    for (const tspan of element.querySelectorAll(':scope > tspan')) tspan.setAttribute('x', String(value));
  }
  function lineX(element, fallback = 0) {
    const parentX = numberAttr(element, 'x', NaN);
    if (Number.isFinite(parentX)) return parentX;
    const first = element.querySelector(':scope > tspan');
    return first ? numberAttr(first, 'x', fallback) : fallback;
  }
  function lineY(element, fallback = 0) {
    const parentY = numberAttr(element, 'y', NaN);
    if (Number.isFinite(parentY)) return parentY;
    const first = element.querySelector(':scope > tspan');
    return first ? numberAttr(first, 'y', fallback) : fallback;
  }
  const tspanPresentationAttributes = [
    'font-family', 'font-size', 'font-weight', 'font-style',
    'fill', 'stroke', 'opacity', 'letter-spacing', 'text-decoration', 'style',
  ];
  function hoistUniformTspanPresentation(element, tspans) {
    if (!tspans.length) return [];
    const hoisted = [];
    for (const attribute of tspanPresentationAttributes) {
      const value = tspans[0].getAttribute(attribute);
      if (value === null || !tspans.every((tspan) => tspan.getAttribute(attribute) === value)) continue;
      // Direct tspans override the parent.  When every line carries the same
      // value, hoisting that value to the parent is appearance-preserving even
      // if the parent previously declared a different inherited default.
      element.setAttribute(attribute, value);
      for (const tspan of tspans) tspan.removeAttribute(attribute);
      hoisted.push(attribute);
    }
    return hoisted;
  }
  function sameComputedPresentation(left, right) {
    const leftStyle = getComputedStyle(left);
    const rightStyle = getComputedStyle(right);
    return [
      'fontFamily', 'fontSize', 'fontWeight', 'fontStyle', 'fill', 'stroke',
      'letterSpacing', 'textDecoration', 'opacity',
    ].every((property) => leftStyle[property] === rightStyle[property]);
  }
  function shiftFirstLineY(element, delta) {
    const first = element.querySelector(':scope > tspan');
    if (first && first.hasAttribute('y')) {
      first.setAttribute('y', String(Number(first.getAttribute('y')) + delta));
    } else {
      element.setAttribute('y', String(numberAttr(element, 'y', 0) + delta));
    }
  }
  function tryResolveTextCollision(element, index, box, allowed) {
    const current = elements[index];
    const otherBoxes = elements
      .map((other, otherIndex) => ({other, otherIndex, box: rootBox(other)}))
      .filter(({other, otherIndex, box: otherBox}) =>
        otherIndex !== index &&
        other.getAttribute('data-pome-allow-overlap') !== 'true' &&
        !(
          current && current.getAttribute('data-pome-collision-scope') &&
          other.getAttribute('data-pome-collision-scope') &&
          current.getAttribute('data-pome-collision-scope') !== other.getAttribute('data-pome-collision-scope')
        ) &&
        intersects(box, otherBox)
      );
    if (!otherBoxes.length) return box;
    // Chromium and PowerPoint do not use identical glyph side bearings.
    // Keep a bounded cross-engine gap when moving a block away from another
    // text block; a 1px Chromium gap can still overlap by about 9px in PPT.
    const gap = 12;
    const candidates = [];
    for (const {box: otherBox} of otherBoxes) {
      candidates.push(
        {dx: 0, dy: otherBox.y + otherBox.height + gap - box.y},
        {dx: 0, dy: otherBox.y - gap - box.y - box.height},
        {dx: otherBox.x + otherBox.width + gap - box.x, dy: 0},
        {dx: otherBox.x - gap - box.x - box.width, dy: 0},
      );
    }
    candidates.sort((left, right) =>
      (Math.abs(left.dx) + Math.abs(left.dy)) - (Math.abs(right.dx) + Math.abs(right.dy))
    );
    for (const candidate of candidates) {
      if (!candidate.dx && !candidate.dy) continue;
      const priorX = lineX(element, 0);
      if (candidate.dx) setLineX(element, priorX + candidate.dx);
      if (candidate.dy) shiftFirstLineY(element, candidate.dy);
      const moved = rootBox(element);
      if (
        fits(moved, allowed) &&
        !collidesWithOtherText(moved, index) &&
        !collidesWithObstacle(moved, index)
      ) return moved;
      if (candidate.dx) setLineX(element, priorX);
      if (candidate.dy) shiftFirstLineY(element, -candidate.dy);
    }
    return box;
  }
  const applied = [];
  elementLoop: for (const index of targetIndexes) {
    const element = elements[index];
    if (!element || element.hasAttribute('transform')) continue;
    let existingTspans = [...element.querySelectorAll(':scope > tspan')];
    const contractMaxLines = Math.max(
      1,
      Math.floor(numberAttr(element, 'data-pome-max-lines', 1)),
    );
    const contractMayWrap = (element.getAttribute('data-pome-wrap') || '').toLowerCase() === 'true';
    const contractRegion = {
      x: numberAttr(element, 'data-pome-region-x', NaN),
      y: numberAttr(element, 'data-pome-region-y', NaN),
      width: numberAttr(element, 'data-pome-region-width', NaN),
      height: numberAttr(element, 'data-pome-region-height', NaN),
    };
    const contractWidth = numberAttr(element, 'data-pome-region-width', NaN);
    const contractBox = rootBox(element);
    const metadataExpansion = Object.values(contractRegion).every(Number.isFinite)
      ? expandAllowedToBox(contractRegion, contractBox)
      : null;
    const requiresLineCompaction =
      Object.values(contractRegion).every(Number.isFinite) &&
      !fits(contractBox, contractRegion) && !metadataExpansion;
    if (
      contractMayWrap && Number.isFinite(contractWidth) &&
      existingTspans.length > contractMaxLines && requiresLineCompaction
    ) {
      let merged = true;
      while (existingTspans.length > contractMaxLines && merged) {
        merged = false;
        for (let tspanIndex = 0; tspanIndex + 1 < existingTspans.length; tspanIndex += 1) {
          const left = existingTspans[tspanIndex];
          const right = existingTspans[tspanIndex + 1];
          if (!sameComputedPresentation(left, right)) continue;
          const leftText = (left.textContent || '').trim();
          const rightText = (right.textContent || '').trim();
          if (!leftText || !rightText) continue;
          const previousText = left.textContent || '';
          left.textContent = `${leftText} ${rightText}`;
          if (left.getComputedTextLength() > contractWidth + 0.75) {
            left.textContent = previousText;
            continue;
          }
          right.remove();
          applied.push({
            domIndex: index,
            text: `${leftText} ${rightText}`,
            action: 'merge-adjacent-compatible-tspan-lines',
            previousLineCount: existingTspans.length,
            lineCount: existingTspans.length - 1,
          });
          existingTspans = [...element.querySelectorAll(':scope > tspan')];
          merged = true;
          break;
        }
      }
    }
    // Executor output often repeats the same presentation attributes on every
    // line while omitting a base y coordinate on the parent <text>.  The
    // browser then lays all such blocks out around y=0.  Hoisting only values
    // that are identical on every direct tspan preserves the appearance and
    // lets the normal line reflow path restore the declared region.  Mixed
    // emphasis remains untouched and therefore still requires an AI retry.
    hoistUniformTspanPresentation(element, existingTspans);
    const hasStyledTspans = existingTspans.some((tspan) =>
      tspan.hasAttribute('class') ||
      tspanPresentationAttributes.some((attribute) => tspan.hasAttribute(attribute))
    );
    const originalText = element.textContent || '';
    const normalizedText = originalText.replace(/\s+/g, ' ').trim();
    if (!normalizedText) continue;
    if (hasStyledTspans) {
      // Mixed rich-text runs must never be flattened or restyled mechanically.
      // A declaration-only correction is still safe when the authored glyphs
      // already fit the canvas without collisions and the region misses their
      // measured bounds by no more than the normal 12px engine-drift budget.
      const richRegion = {
        x: numberAttr(element, 'data-pome-region-x', NaN),
        y: numberAttr(element, 'data-pome-region-y', NaN),
        width: numberAttr(element, 'data-pome-region-width', NaN),
        height: numberAttr(element, 'data-pome-region-height', NaN),
      };
      if (Object.values(richRegion).every((value) => Number.isFinite(value))) {
        const richBox = rootBox(element);
        const richCandidate = {
          x: Math.min(richRegion.x, richBox.x),
          y: Math.min(richRegion.y, richBox.y),
          width: Math.max(richRegion.x + richRegion.width, richBox.x + richBox.width) -
            Math.min(richRegion.x, richBox.x),
          height: Math.max(richRegion.y + richRegion.height, richBox.y + richBox.height) -
            Math.min(richRegion.y, richBox.y),
        };
        const richExpansion = Math.max(
          richRegion.x - richCandidate.x,
          richRegion.y - richCandidate.y,
          richCandidate.x + richCandidate.width - richRegion.x - richRegion.width,
          richCandidate.y + richCandidate.height - richRegion.y - richRegion.height,
        );
        if (
          richExpansion > 0 && richExpansion <= 12 &&
          richCandidate.x >= 0 && richCandidate.y >= 0 &&
          richCandidate.x + richCandidate.width <= 1280 &&
          richCandidate.y + richCandidate.height <= 720 &&
          !collidesWithOtherText(richBox, index) &&
          !collidesWithObstacle(richBox, index)
        ) {
          element.setAttribute('data-pome-region-x', String(richCandidate.x));
          element.setAttribute('data-pome-region-y', String(richCandidate.y));
          element.setAttribute('data-pome-region-width', String(richCandidate.width));
          element.setAttribute('data-pome-region-height', String(richCandidate.height));
          applied.push({
            domIndex: index,
            text: normalizedText,
            action: 'expand-rich-text-region',
            expansion: richExpansion,
            region: richCandidate,
          });
        }
      }
      continue;
    }
    const missingRegionMetadata =
      !element.getAttribute('data-pome-role') ||
      !element.getAttribute('data-pome-region-id') ||
      ['x', 'y', 'width', 'height'].some((field) =>
        !element.hasAttribute(`data-pome-region-${field}`)
      );
    if (missingRegionMetadata) {
      // Text inside an explicitly marked semantic obstacle already has an
      // unambiguous owning node.  Declaring a region around its measured bbox
      // is a safe metadata repair: it does not move, resize, delete, or rewrite
      // visible content.  Unmarked normal text outside such a node remains a
      // hard error because the checker must not guess its layout ownership.
      const glyphs = Array.from(normalizedText);
      const textBox = rootBox(element);
      const canvas = {x: 0, y: 0, width: 1280, height: 720};
      const style = getComputedStyle(element);
      const fontSize = Number.parseFloat(style.fontSize);
      const semanticContainer = element.parentElement && element.parentElement.closest(
        '[data-pome-obstacle="true"][data-pome-region-id]'
      );
      const semanticRegionId = semanticContainer && semanticContainer.getAttribute('data-pome-region-id');
      const canDeclareContainerText = Boolean(semanticContainer && semanticRegionId);
      const canDeclareDecorativeGlyph = glyphs.length <= 2;
      // Timeline ticks and other compact, isolated labels are frequently emitted
      // as plain <text> nodes.  Their own measured ink box is an unambiguous
      // region: adding metadata around it does not move text or infer ownership
      // of a larger body block.  Keep this deliberately narrow so unmarked body
      // copy still fails strict validation and must be regenerated.
      const canDeclareIsolatedLabel =
        !canDeclareContainerText && !canDeclareDecorativeGlyph &&
        Number.isFinite(fontSize) && fontSize <= 16 &&
        glyphs.length <= 24 && existingTspans.length <= 1 &&
        textBox.width <= 240 && textBox.height <= 32;
      if (
        (canDeclareContainerText || canDeclareDecorativeGlyph || canDeclareIsolatedLabel) &&
        fits(textBox, canvas) &&
        !collidesWithOtherText(textBox, index) &&
        !collidesWithObstacle(textBox, index)
      ) {
        const parsedWeight = Number.parseInt(style.fontWeight, 10);
        const fontWeight = Number.isFinite(parsedWeight) ? parsedWeight : 400;
        let role = 'body';
        if (canDeclareDecorativeGlyph && !canDeclareContainerText) role = 'label';
        else if (canDeclareIsolatedLabel) role = fontSize <= 11 ? 'caption' : 'label';
        else if (fontSize >= 34) role = 'title';
        else if (fontSize >= 24) role = 'subtitle';
        else if (fontSize <= 11) role = 'caption';
        else if (fontWeight >= 600 && glyphs.length <= 20) role = 'label';

        const padding = canDeclareContainerText ? 2 : 0;
        const region = {
          x: Math.max(0, textBox.x - padding),
          y: Math.max(0, textBox.y - padding),
          width: Math.min(1280, textBox.x + textBox.width + padding) - Math.max(0, textBox.x - padding),
          height: Math.min(720, textBox.y + textBox.height + padding) - Math.max(0, textBox.y - padding),
        };
        const lineCount = Math.max(1, existingTspans.reduce((count, tspan, tspanIndex) => {
          const startsLine = tspanIndex === 0 || tspan.hasAttribute('x') ||
            tspan.hasAttribute('y') || tspan.hasAttribute('dy');
          return count + (startsLine ? 1 : 0);
        }, existingTspans.length ? 0 : 1));

        element.setAttribute('data-pome-role', role);
        element.setAttribute(
          'data-pome-region-id',
          canDeclareContainerText
            ? semanticRegionId
            : canDeclareIsolatedLabel
              ? `auto-isolated-label-${index + 1}`
              : `auto-decorative-glyph-${index + 1}`
        );
        element.setAttribute('data-pome-region-x', String(region.x));
        element.setAttribute('data-pome-region-y', String(region.y));
        element.setAttribute('data-pome-region-width', String(region.width));
        element.setAttribute('data-pome-region-height', String(region.height));
        element.setAttribute('data-pome-min-font-size', String(fontSize));
        element.setAttribute('data-pome-wrap', 'false');
        element.setAttribute('data-pome-max-lines', String(lineCount));
        element.setAttribute('data-pome-safe-padding', String(padding));
        applied.push({
          domIndex: index,
          text: normalizedText,
          action: canDeclareContainerText
            ? 'declare-container-text-region'
            : canDeclareIsolatedLabel
              ? 'declare-isolated-label-region'
              : 'declare-decorative-glyph-region',
          region,
        });
      }
      continue;
    }
    const region = {
      x: numberAttr(element, 'data-pome-region-x', NaN),
      y: numberAttr(element, 'data-pome-region-y', NaN),
      width: numberAttr(element, 'data-pome-region-width', NaN),
      height: numberAttr(element, 'data-pome-region-height', NaN),
    };
    if (Object.values(region).some((value) => !Number.isFinite(value))) continue;
    const padding = numberAttr(element, 'data-pome-safe-padding', 8);
    const clampedRegionWidth = Math.min(1280, Math.max(0, region.width));
    const clampedRegionHeight = Math.min(720, Math.max(0, region.height));
    let allowed = {
      x: Math.min(Math.max(0, region.x), 1280 - clampedRegionWidth),
      y: Math.min(Math.max(0, region.y), 720 - clampedRegionHeight),
      width: clampedRegionWidth,
      height: clampedRegionHeight,
    };
    if (
      Math.abs(allowed.x - region.x) > 0.01 || Math.abs(allowed.y - region.y) > 0.01 ||
      Math.abs(allowed.width - region.width) > 0.01 || Math.abs(allowed.height - region.height) > 0.01
    ) {
      element.setAttribute('data-pome-region-x', String(allowed.x));
      element.setAttribute('data-pome-region-y', String(allowed.y));
      element.setAttribute('data-pome-region-width', String(allowed.width));
      element.setAttribute('data-pome-region-height', String(allowed.height));
      applied.push({domIndex: index, text: normalizedText, action: 'clamp-region', region: allowed});
    }
    const originalFontSize = Number.parseFloat(getComputedStyle(element).fontSize);
    const minFontSize = Math.min(originalFontSize, numberAttr(element, 'data-pome-min-font-size', originalFontSize));
    const safeMinFontSize = Math.max(minFontSize, originalFontSize * 0.85);
    const mayWrap = (element.getAttribute('data-pome-wrap') || '').toLowerCase() === 'true';
    let maxLines = Math.max(1, Math.floor(numberAttr(element, 'data-pome-max-lines', 1)));
    const originalX = lineX(element, region.x);
    const originalY = lineY(element, region.y + Math.max(1, originalFontSize));
    const originalMarkup = element.innerHTML;
    const existingBox = rootBox(element);

    // Preserve authored tspan boundaries before flattening/re-wrapping them.
    // Uniform multiline blocks can usually be repaired by establishing their
    // baseline, tightening line height, and applying a very small local font
    // reduction. Mixed-size rich text is deliberately excluded above.
    if (existingTspans.length) {
      for (let fontSize = originalFontSize; fontSize + 0.01 >= safeMinFontSize; fontSize -= 0.5) {
        for (const lineHeightScale of [null, 1.15, 1.08, 1.0]) {
          element.innerHTML = originalMarkup;
          element.setAttribute('font-size', String(fontSize));
          setLineX(element, originalX);
          const restoredFirst = element.querySelector(':scope > tspan');
          if (restoredFirst) restoredFirst.setAttribute('y', String(originalY));
          const restoredTspans = [...element.querySelectorAll(':scope > tspan')];
          if (lineHeightScale !== null) {
            restoredTspans.slice(1).forEach((tspan) => {
              if (tspan.hasAttribute('dy')) {
                const currentDy = Math.abs(numberAttr(tspan, 'dy', fontSize * lineHeightScale));
                tspan.setAttribute('dy', String(Math.min(currentDy, fontSize * lineHeightScale)));
              }
            });
          }
          let box = rootBox(element);
          let expandedRegion = expandAllowedToBox(allowed, box);
          let candidateAllowed = expandedRegion ? expandedRegion.region : {...allowed};
          let dx = 0;
          let dy = 0;
          if (box.x < candidateAllowed.x) dx = candidateAllowed.x - box.x;
          else if (box.x + box.width > candidateAllowed.x + candidateAllowed.width) {
            dx = candidateAllowed.x + candidateAllowed.width - box.x - box.width;
          }
          if (box.y < candidateAllowed.y) dy = candidateAllowed.y - box.y;
          else if (box.y + box.height > candidateAllowed.y + candidateAllowed.height) {
            dy = candidateAllowed.y + candidateAllowed.height - box.y - box.height;
          }
          if (dx) setLineX(element, lineX(element, originalX) + dx);
          if (dy) shiftFirstLineY(element, dy);
          box = rootBox(element);
          if (!fits(box, candidateAllowed)) {
            expandedRegion = expandAllowedToBox(candidateAllowed, box);
            if (expandedRegion) candidateAllowed = expandedRegion.region;
          }
          if (fits(box, candidateAllowed) && collidesWithOtherText(box, index)) {
            box = tryResolveTextCollision(element, index, box, candidateAllowed);
          }
          if (
            fits(box, candidateAllowed) && !collidesWithOtherText(box, index) &&
            !collidesWithObstacle(box, index)
          ) {
            const effectiveLineHeight = lineHeightScale === null
              ? numberAttr(element, 'data-pome-line-height', fontSize * 1.2)
              : fontSize * lineHeightScale;
            element.setAttribute('data-pome-line-height', String(effectiveLineHeight));
            if (expandedRegion) {
              const expanded = expandedRegion.region;
              element.setAttribute('data-pome-region-x', String(expanded.x));
              element.setAttribute('data-pome-region-y', String(expanded.y));
              element.setAttribute('data-pome-region-width', String(expanded.width));
              element.setAttribute('data-pome-region-height', String(expanded.height));
              allowed = expanded;
              applied.push({
                domIndex: index,
                text: normalizedText,
                action: 'expand-region-for-existing-lines',
                expansion: expandedRegion.expansion,
                region: expanded,
              });
            }
            if (restoredTspans.length > maxLines) {
              element.setAttribute('data-pome-max-lines', String(restoredTspans.length));
              applied.push({
                domIndex: index,
                text: normalizedText,
                action: 'correct-authored-line-count-contract',
                previousMaxLines: maxLines,
                maxLines: restoredTspans.length,
              });
            }
            applied.push({
              domIndex: index,
              text: normalizedText,
              action: 'fit-existing-tspan-lines',
              fontSize,
              lineHeight: effectiveLineHeight,
              lineCount: restoredTspans.length,
            });
            continue elementLoop;
          }
        }
      }
      element.innerHTML = originalMarkup;
      element.setAttribute('font-size', String(originalFontSize));
      setLineX(element, originalX);
      const restoredFirst = element.querySelector(':scope > tspan');
      if (restoredFirst) restoredFirst.setAttribute('y', String(originalY));
    }

    // Some Executor pages mark a long body as wrappable but still declare a
    // one-line, 24px-high region.  If the unchanged text is genuinely wider
    // than that region and a second line fits in empty local space, promote only
    // this shallow body region to two lines.  The normal reflow below must still
    // prove the final glyph box has no text/obstacle collision.
    const initialRole = (element.getAttribute('data-pome-role') || '').toLowerCase();
    if (
      (initialRole === 'body' || initialRole === 'caption') && mayWrap &&
      maxLines === 1 && Number.isFinite(originalFontSize) && originalFontSize <= 18 &&
      existingBox.width > allowed.width + 0.75 &&
      allowed.height <= originalFontSize * 1.75
    ) {
      const twoLineRegion = {
        ...allowed,
        height: Math.max(allowed.height, Math.ceil(originalFontSize * 2.8)),
      };
      if (
        twoLineRegion.height <= 56 &&
        twoLineRegion.y + twoLineRegion.height <= 720 &&
        !collidesWithOtherText(twoLineRegion, index) &&
        !collidesWithObstacle(twoLineRegion, index)
      ) {
        element.setAttribute('data-pome-region-height', String(twoLineRegion.height));
        element.setAttribute('data-pome-max-lines', '2');
        allowed = twoLineRegion;
        maxLines = 2;
        applied.push({
          domIndex: index,
          text: normalizedText,
          action: 'promote-shallow-body-to-two-lines',
          region: twoLineRegion,
        });
      }
    }
    // Prefer correcting a slightly undersized region declaration over moving or
    // shrinking text that is already safely placed.  This is intentionally
    // bounded and collision checked: a genuinely overfull block must still be
    // retried instead of obtaining an arbitrarily larger region.
    if (!fits(existingBox, allowed)) {
      // A common Executor mistake is to use the text anchor itself as the
      // region's left edge.  For end/middle anchored text, derive the region
      // from the unchanged anchor and declared size.  This can correct a large
      // metadata displacement without moving the visible label or relaxing
      // collision/canvas checks.
      const textAnchor = (getComputedStyle(element).textAnchor ||
        element.getAttribute('text-anchor') || 'start').toLowerCase();
      const anchorX = numberAttr(element, 'x', NaN);
      let anchorAlignedRegion = null;
      if (Number.isFinite(anchorX)) {
        if (textAnchor === 'end') {
          anchorAlignedRegion = {...allowed, x: anchorX - allowed.width};
        } else if (textAnchor === 'middle') {
          anchorAlignedRegion = {...allowed, x: anchorX - allowed.width / 2};
        }
      }
      if (
        anchorAlignedRegion && anchorAlignedRegion.x >= 0 &&
        anchorAlignedRegion.x + anchorAlignedRegion.width <= 1280 &&
        anchorAlignedRegion.y >= 0 &&
        anchorAlignedRegion.y + anchorAlignedRegion.height <= 720 &&
        fits(existingBox, anchorAlignedRegion) &&
        !collidesWithOtherText(existingBox, index) &&
        !collidesWithObstacle(existingBox, index)
      ) {
        element.setAttribute('data-pome-region-x', String(anchorAlignedRegion.x));
        element.setAttribute('data-pome-region-y', String(anchorAlignedRegion.y));
        element.setAttribute('data-pome-region-width', String(anchorAlignedRegion.width));
        element.setAttribute('data-pome-region-height', String(anchorAlignedRegion.height));
        applied.push({
          domIndex: index,
          text: normalizedText,
          action: 'align-region-to-text-anchor',
          textAnchor,
          region: anchorAlignedRegion,
        });
        continue;
      }
      // Compact labels and metrics are often positioned correctly while their
      // metadata rectangle is offset.  Relocate (and, for metrics/footers,
      // minimally grow) that invisible rectangle before considering any visible
      // x/y movement.  Body/title blocks are intentionally excluded.
      const metadataFirstRole = (element.getAttribute('data-pome-role') || '').toLowerCase();
      const mayRelocateMetadataFirst = ['label', 'metric', 'unit', 'footer', 'caption']
        .includes(metadataFirstRole);
      const mayGrowCompactRegion = ['metric', 'unit', 'footer'].includes(metadataFirstRole);
      const compactWidth = mayGrowCompactRegion
        ? Math.max(allowed.width, existingBox.width)
        : allowed.width;
      const compactHeight = mayGrowCompactRegion
        ? Math.max(allowed.height, existingBox.height)
        : allowed.height;
      const compactRegion = {
        x: Math.min(1280 - compactWidth, Math.max(0, existingBox.x - Math.max(0, compactWidth - existingBox.width) / 2)),
        y: Math.min(720 - compactHeight, Math.max(0, existingBox.y - Math.max(0, compactHeight - existingBox.height) / 2)),
        width: compactWidth,
        height: compactHeight,
      };
      const compactCenterShift = Math.hypot(
        compactRegion.x + compactRegion.width / 2 - (allowed.x + allowed.width / 2),
        compactRegion.y + compactRegion.height / 2 - (allowed.y + allowed.height / 2),
      );
      if (
        mayRelocateMetadataFirst && compactCenterShift > 0.75 && compactCenterShift <= 120 &&
        fits(existingBox, compactRegion) &&
        !collidesWithOtherText(existingBox, index) &&
        !collidesWithObstacle(existingBox, index)
      ) {
        element.setAttribute('data-pome-region-x', String(compactRegion.x));
        element.setAttribute('data-pome-region-y', String(compactRegion.y));
        element.setAttribute('data-pome-region-width', String(compactRegion.width));
        element.setAttribute('data-pome-region-height', String(compactRegion.height));
        applied.push({
          domIndex: index,
          text: normalizedText,
          action: 'relocate-compact-region-before-text',
          centerShift: compactCenterShift,
          region: compactRegion,
        });
        continue;
      }
      const regionCandidate = {
        x: Math.min(allowed.x, existingBox.x),
        y: Math.min(allowed.y, existingBox.y),
        width: Math.max(allowed.x + allowed.width, existingBox.x + existingBox.width) -
          Math.min(allowed.x, existingBox.x),
        height: Math.max(allowed.y + allowed.height, existingBox.y + existingBox.height) -
          Math.min(allowed.y, existingBox.y),
      };
      const regionExpansion = Math.max(
        allowed.x - regionCandidate.x,
        allowed.y - regionCandidate.y,
        regionCandidate.x + regionCandidate.width - allowed.x - allowed.width,
        regionCandidate.y + regionCandidate.height - allowed.y - allowed.height,
      );
      const regionMaxSafeExpansion = Math.max(
        40,
        Math.min(120, Math.max(allowed.width, allowed.height) * 0.5),
      );
      const regionRole = (element.getAttribute('data-pome-role') || '').toLowerCase();
      const isSingleLineHeading =
        ['title', 'subtitle'].includes(regionRole) && !mayWrap && existingTspans.length <= 1;
      const maxMetadataExpansion = regionRole === 'footer'
        ? regionMaxSafeExpansion
        : isSingleLineHeading
          ? Math.min(160, Math.max(12, allowed.width * 0.15))
          : Math.min(12, regionMaxSafeExpansion);
      if (
        regionExpansion > 0 && regionExpansion <= maxMetadataExpansion &&
        regionCandidate.x >= 0 && regionCandidate.y >= 0 &&
        regionCandidate.x + regionCandidate.width <= 1280 &&
        regionCandidate.y + regionCandidate.height <= 720 &&
        !collidesWithOtherText(existingBox, index) &&
        !collidesWithObstacle(existingBox, index)
      ) {
        element.setAttribute('data-pome-region-x', String(regionCandidate.x));
        element.setAttribute('data-pome-region-y', String(regionCandidate.y));
        element.setAttribute('data-pome-region-width', String(regionCandidate.width));
        element.setAttribute('data-pome-region-height', String(regionCandidate.height));
        applied.push({
          domIndex: index,
          text: normalizedText,
          action: 'expand-region',
          expansion: regionExpansion,
          region: regionCandidate,
        });
        continue;
      }
    }
    let fixed = false;
    for (let fontSize = originalFontSize; fontSize + 0.01 >= safeMinFontSize && !fixed; fontSize -= 0.5) {
      for (const lineHeightScale of [1.15, 1.08, 1.0]) {
        element.setAttribute('font-size', String(fontSize));
        element.setAttribute('x', String(originalX));
        element.setAttribute('y', String(originalY));
        const probe = element.cloneNode(false);
        probe.setAttribute('visibility', 'hidden');
        root.appendChild(probe);
        const lines = [];
        if (mayWrap) {
          let line = '';
          for (const char of Array.from(normalizedText)) {
            const candidate = line + char;
            probe.textContent = candidate;
            if (line && probe.getComputedTextLength() > allowed.width) {
              lines.push(line);
              line = char;
            } else {
              line = candidate;
            }
          }
          if (line) lines.push(line);
        } else {
          lines.push(normalizedText);
        }
        probe.remove();
        if (!lines.length || lines.length > maxLines) continue;
        element.textContent = '';
        lines.forEach((line, lineIndex) => {
          const tspan = document.createElementNS('http://www.w3.org/2000/svg', 'tspan');
          tspan.setAttribute('x', String(originalX));
          if (lineIndex === 0) tspan.setAttribute('y', String(originalY));
          else tspan.setAttribute('dy', String(fontSize * lineHeightScale));
          tspan.textContent = line;
          element.appendChild(tspan);
        });
        let box = rootBox(element);
        let dx = 0;
        let dy = 0;
        if (box.x < allowed.x) dx = allowed.x - box.x;
        else if (box.x + box.width > allowed.x + allowed.width) dx = allowed.x + allowed.width - box.x - box.width;
        if (box.y < allowed.y) dy = allowed.y - box.y;
        else if (box.y + box.height > allowed.y + allowed.height) dy = allowed.y + allowed.height - box.y - box.height;
        if (dx) setLineX(element, originalX + dx);
        if (dy) shiftFirstLineY(element, dy);
        box = rootBox(element);
        let fittingRegion = allowed;
        let localRegionExpansion = null;
        if (!fits(box, fittingRegion)) {
          const candidate = {
            x: Math.min(fittingRegion.x, box.x),
            y: Math.min(fittingRegion.y, box.y),
            width: Math.max(fittingRegion.x + fittingRegion.width, box.x + box.width) -
              Math.min(fittingRegion.x, box.x),
            height: Math.max(fittingRegion.y + fittingRegion.height, box.y + box.height) -
              Math.min(fittingRegion.y, box.y),
          };
          const expansion = Math.max(
            fittingRegion.x - candidate.x,
            fittingRegion.y - candidate.y,
            candidate.x + candidate.width - fittingRegion.x - fittingRegion.width,
            candidate.y + candidate.height - fittingRegion.y - fittingRegion.height,
          );
          if (
            expansion > 0 && expansion <= 12 && candidate.x >= 0 && candidate.y >= 0 &&
            candidate.x + candidate.width <= 1280 && candidate.y + candidate.height <= 720 &&
            fits(box, candidate)
          ) {
            fittingRegion = candidate;
            localRegionExpansion = {region: candidate, expansion};
          }
        }
        if (fits(box, fittingRegion) && collidesWithOtherText(box, index)) {
          box = tryResolveTextCollision(element, index, box, fittingRegion);
        }
        if (
          fits(box, fittingRegion) && !collidesWithOtherText(box, index) &&
          !collidesWithObstacle(box, index)
        ) {
          if (localRegionExpansion) {
            allowed = fittingRegion;
            element.setAttribute('data-pome-region-x', String(allowed.x));
            element.setAttribute('data-pome-region-y', String(allowed.y));
            element.setAttribute('data-pome-region-width', String(allowed.width));
            element.setAttribute('data-pome-region-height', String(allowed.height));
            applied.push({
              domIndex: index,
              text: normalizedText,
              action: 'expand-region-after-reflow',
              expansion: localRegionExpansion.expansion,
              region: allowed,
            });
          }
          fixed = true;
          applied.push({domIndex: index, text: normalizedText, fontSize, lineCount: lines.length});
          break;
        }
      }
    }
    if (!fixed) {
      element.innerHTML = originalMarkup;
      element.setAttribute('x', String(originalX));
      element.setAttribute('y', String(originalY));
      element.setAttribute('font-size', String(originalFontSize));
      const box = rootBox(element);
      // Relocation repairs wrong metadata coordinates only.  It must not make
      // an actually overfull text block pass by silently enlarging its region.
      // A relocated region must still contain the measured glyph box.  Keeping
      // an undersized width here made end-anchored footers impossible to fix:
      // the region moved to the correct x coordinate but remained narrower
      // than the unchanged text, causing the whole transactional page repair
      // to be rejected.
      const relocatedRole = (element.getAttribute('data-pome-role') || '').toLowerCase();
      const mayRelocateOwnedHeading =
        ['title', 'subtitle'].includes(relocatedRole) &&
        Boolean(element.getAttribute('data-pome-owner')) &&
        !mayWrap && existingTspans.length <= 1;
      const mayGrowRelocatedRegion =
        relocatedRole === 'footer' || relocatedRole === 'metric' || mayRelocateOwnedHeading;
      const relocatedWidth = mayGrowRelocatedRegion
        ? Math.max(allowed.width, box.width)
        : allowed.width;
      const relocatedHeight = mayGrowRelocatedRegion
        ? Math.max(allowed.height, box.height)
        : allowed.height;
      const relocated = {
        x: Math.min(1280 - relocatedWidth, Math.max(0, box.x - Math.max(0, relocatedWidth - box.width) / 2)),
        y: Math.min(720 - relocatedHeight, Math.max(0, box.y - Math.max(0, relocatedHeight - box.height) / 2)),
        width: relocatedWidth,
        height: relocatedHeight,
      };
      const regionCenterShift = Math.hypot(
        relocated.x + relocated.width / 2 - (region.x + region.width / 2),
        relocated.y + relocated.height / 2 - (region.y + region.height / 2),
      );
      const relocatedTextCollision = collidesWithOtherText(box, index);
      const relocatedObstacleCollision = collidesWithObstacle(relocated, index);
      const maximumMetadataShift = mayRelocateOwnedHeading ? 640 : 120;
      if (
        regionCenterShift > 0.75 && regionCenterShift <= maximumMetadataShift &&
        relocated.x >= 0 && relocated.y >= 0 &&
        relocated.x + relocated.width <= 1280 && relocated.y + relocated.height <= 720 &&
        fits(box, relocated) &&
        !relocatedTextCollision && !relocatedObstacleCollision
      ) {
        element.setAttribute('data-pome-region-x', String(relocated.x));
        element.setAttribute('data-pome-region-y', String(relocated.y));
        element.setAttribute('data-pome-region-width', String(relocated.width));
        element.setAttribute('data-pome-region-height', String(relocated.height));
        fixed = true;
        applied.push({
          domIndex: index,
          text: normalizedText,
          action: 'relocate-region',
          centerShift: regionCenterShift,
          region: relocated,
        });
      }
      if (fixed) continue;
      const candidate = {
        x: Math.min(region.x, box.x),
        y: Math.min(region.y, box.y),
        width: Math.max(region.x + region.width, box.x + box.width) - Math.min(region.x, box.x),
        height: Math.max(region.y + region.height, box.y + box.height) - Math.min(region.y, box.y),
      };
      const expansion = Math.max(
        region.x - candidate.x,
        region.y - candidate.y,
        candidate.x + candidate.width - region.x - region.width,
        candidate.y + candidate.height - region.y - region.height,
      );
      const otherTextCollision = elements.some((other, otherIndex) =>
        otherIndex !== index &&
        other.getAttribute('data-pome-allow-overlap') !== 'true' &&
        intersects(candidate, rootBox(other))
      );
      const obstacleCollision = collidesWithObstacle(candidate, index);
      const maxSafeExpansion = Math.max(12, Math.min(120, Math.max(region.width, region.height) * 0.5));
      if (
        expansion > 0 && expansion <= maxSafeExpansion &&
        candidate.x >= 0 && candidate.y >= 0 &&
        candidate.x + candidate.width <= 1280 && candidate.y + candidate.height <= 720 &&
        !otherTextCollision && !obstacleCollision
      ) {
        element.setAttribute('data-pome-region-x', String(candidate.x));
        element.setAttribute('data-pome-region-y', String(candidate.y));
        element.setAttribute('data-pome-region-width', String(candidate.width));
        element.setAttribute('data-pome-region-height', String(candidate.height));
        fixed = true;
        applied.push({
          domIndex: index,
          text: normalizedText,
          action: 'expand-region',
          expansion,
          region: candidate,
        });
      }
    }
  }
  return {applied, svg: new XMLSerializer().serializeToString(root)};
})
"""


BOOTSTRAP_REGION_SCRIPT = r"""
(async () => {
  await document.fonts.ready;
  const root = document.documentElement;
  const rootInverse = root.getScreenCTM().inverse();
  function rootBox(element) {
    const box = element.getBBox();
    const matrix = rootInverse.multiply(element.getScreenCTM());
    const points = [
      new DOMPoint(box.x, box.y), new DOMPoint(box.x + box.width, box.y),
      new DOMPoint(box.x, box.y + box.height), new DOMPoint(box.x + box.width, box.y + box.height),
    ].map((point) => point.matrixTransform(matrix));
    const xs = points.map((point) => point.x);
    const ys = points.map((point) => point.y);
    const x = Math.min(...xs);
    const y = Math.min(...ys);
    return {x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y};
  }
  const texts = [...root.querySelectorAll('text')];
  texts.forEach((element, index) => {
    const existingRegionId = element.getAttribute('data-pome-region-id') || '';
    if (element.hasAttribute('data-pome-role') && !existingRegionId.startsWith('v2-migration-text-')) return;
    const box = rootBox(element);
    const style = getComputedStyle(element);
    const fontSize = Number.parseFloat(style.fontSize) || 16;
    const weight = Number.parseInt(style.fontWeight, 10) || 400;
    const text = (element.textContent || '').replace(/\s+/g, ' ').trim();
    let role = 'body';
    if (fontSize >= 34) role = 'title';
    else if (fontSize >= 22) role = 'subtitle';
    else if (fontSize <= 11) role = 'caption';
    else if (weight >= 600 && text.length <= 20) role = 'label';
    // Pre-v3 artifacts did not allocate semantic regions.  The migration box
    // therefore includes a one-time font-engine drift allowance; production
    // v3 pages never use this bootstrap path and must declare real regions.
    const regionPadding = 36;
    const safePadding = role === 'title' ? 10 : role === 'body' ? 8 : 6;
    const region = {
      x: Math.max(0, box.x - regionPadding),
      y: Math.max(0, box.y - regionPadding),
      width: Math.min(1280, box.x + box.width + regionPadding) - Math.max(0, box.x - regionPadding),
      height: Math.min(720, box.y + box.height + regionPadding) - Math.max(0, box.y - regionPadding),
    };
    element.setAttribute('data-pome-role', role);
    element.setAttribute('data-pome-region-id', `v2-migration-text-${index + 1}`);
    element.setAttribute('data-pome-region-x', region.x.toFixed(2));
    element.setAttribute('data-pome-region-y', region.y.toFixed(2));
    element.setAttribute('data-pome-region-width', region.width.toFixed(2));
    element.setAttribute('data-pome-region-height', region.height.toFixed(2));
    element.setAttribute('data-pome-min-font-size', String(Math.max(9, fontSize - 2)));
    element.setAttribute('data-pome-wrap', text.length > 18 ? 'true' : 'false');
    element.setAttribute('data-pome-max-lines', String(Math.max(1, Math.min(3, element.querySelectorAll(':scope > tspan').length || 1))));
    element.setAttribute('data-pome-safe-padding', String(safePadding));
  });
  return {count: texts.length, svg: new XMLSerializer().serializeToString(root)};
})()
"""


@dataclass
class BrowserSession:
    process: subprocess.Popen[str]
    socket: Any
    profile: Path
    next_id: int = 1

    def command(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        command_id = self.next_id
        self.next_id += 1
        self.socket.send(json.dumps({"id": command_id, "method": method, "params": params or {}}))
        while True:
            message = json.loads(self.socket.recv())
            if message.get("id") == command_id:
                if "error" in message:
                    raise RuntimeError(f"Chrome DevTools {method} failed: {message['error']}")
                return message.get("result", {})

    def close(self) -> None:
        try:
            self.socket.close()
        finally:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
            # Chrome may leave Crashpad briefly holding a dump file.  Cleanup is
            # best-effort and must never hide a completed geometry result.
            shutil.rmtree(self.profile, ignore_errors=True)


def _browser_candidates() -> list[str]:
    candidates = [
        os.environ.get("POME_CHROMIUM_PATH", ""),
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        shutil.which("google-chrome") or "",
        shutil.which("chromium") or "",
        shutil.which("chromium-browser") or "",
    ]
    return [candidate for candidate in candidates if candidate and Path(candidate).is_file()]


def open_browser() -> BrowserSession:
    candidates = _browser_candidates()
    if not candidates:
        raise RuntimeError("未找到可用于 SVG 真实字体度量的 Chrome/Edge")
    profile = Path(tempfile.mkdtemp(prefix="pome-native-svg-geometry-"))
    creation_flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
    process = subprocess.Popen(
        [
            candidates[0],
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            "--allow-file-access-from-files",
            "--remote-debugging-port=0",
            f"--user-data-dir={profile}",
            "about:blank",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        text=True,
        creationflags=creation_flags,
    )
    port_file = profile / "DevToolsActivePort"
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline and not port_file.is_file():
        if process.poll() is not None:
            shutil.rmtree(profile, ignore_errors=True)
            raise RuntimeError("Chrome/Edge 在建立 DevTools 会话前退出")
        time.sleep(0.05)
    if not port_file.is_file():
        process.terminate()
        shutil.rmtree(profile, ignore_errors=True)
        raise RuntimeError("等待 Chrome/Edge DevTools 端口超时")
    port = port_file.read_text(encoding="utf-8").splitlines()[0]
    request = urllib.request.Request(f"http://127.0.0.1:{port}/json/new?about:blank", method="PUT")
    with urllib.request.urlopen(request, timeout=5) as response:
        target = json.load(response)
    socket = connect(target["webSocketDebuggerUrl"], open_timeout=5, close_timeout=2)
    session = BrowserSession(process=process, socket=socket, profile=profile)
    session.command("Page.enable")
    session.command("Runtime.enable")
    return session


def measure_svg(session: BrowserSession, svg_path: Path) -> dict[str, Any]:
    uri = svg_path.resolve().as_uri()
    session.command("Page.navigate", {"url": uri})
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        ready = session.command(
            "Runtime.evaluate",
            {"expression": "document.readyState", "returnByValue": True},
        )
        if ready.get("result", {}).get("value") == "complete":
            break
        time.sleep(0.05)
    else:
        raise RuntimeError(f"加载 SVG 超时: {svg_path}")
    evaluated = session.command(
        "Runtime.evaluate",
        {
            "expression": MEASURE_SCRIPT,
            "awaitPromise": True,
            "returnByValue": True,
        },
    )
    result = evaluated.get("result", {})
    if result.get("subtype") == "error" or "value" not in result:
        raise RuntimeError(f"Chromium 无法测量 SVG: {result.get('description', result)}")
    measured = result["value"]
    measured["svgPath"] = str(svg_path.resolve())
    return measured


def attempt_safe_fix(
    session: BrowserSession,
    svg_path: Path,
    before: dict[str, Any],
    before_visible_text: list[str],
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    def target_indexes(report: dict[str, Any]) -> list[int]:
        targets: set[int] = set()
        for issue in report.get("hardErrors", []):
            if issue.get("rule") in {
                "text_outside_canvas",
                "text_region_outside_canvas",
                "text_outside_declared_region",
                "text_exceeds_max_lines",
                "text_text_overlap",
                "text_obstacle_overlap",
                "missing_text_region_metadata",
                "missing_first_line_baseline",
            } and issue.get("domIndex") is not None:
                targets.add(int(issue["domIndex"]))
            collision_index = (issue.get("collision") or {}).get("domIndex")
            if issue.get("rule") == "text_text_overlap" and collision_index is not None:
                targets.add(int(collision_index))
        return sorted(targets)

    # A malformed block near y=0 can initially prevent an otherwise safe title
    # repair.  Once that block is restored to its declared region, the title is
    # repairable.  Re-measure a bounded number of times so dependencies settle,
    # while retaining the all-or-nothing page transaction below.
    current = before
    after = before
    measured: dict[str, Any] | None = None
    applied: list[dict[str, Any]] = []
    serialized_svg: str | None = None

    # Normalize contract metadata and missing multiline baselines for every
    # affected block in one browser transaction. Doing this before per-element
    # collision resolution removes ordering dependencies when several malformed
    # blocks all render around SVG's implicit y=0 baseline.
    initial_targets = target_indexes(before)
    if initial_targets:
        expression = f"({CONTRACT_NORMALIZE_SCRIPT})({json.dumps(initial_targets)})"
        evaluated = session.command(
            "Runtime.evaluate",
            {"expression": expression, "awaitPromise": True, "returnByValue": True},
        )
        value = evaluated.get("result", {}).get("value")
        if isinstance(value, dict) and isinstance(value.get("svg"), str):
            normalization_applied = (
                value.get("applied") if isinstance(value.get("applied"), list) else []
            )
            if normalization_applied:
                applied.extend(normalization_applied)
                serialized_svg = value["svg"]
                measured = measure_current_document(session, svg_path)
                current = classify_measurements(measured, require_markers=True)
                after = current

    for _ in range(3):
        targets = target_indexes(current)
        if not targets:
            break
        expression = f"({SAFE_FIX_SCRIPT})({json.dumps(targets)})"
        evaluated = session.command(
            "Runtime.evaluate",
            {"expression": expression, "awaitPromise": True, "returnByValue": True},
        )
        value = evaluated.get("result", {}).get("value")
        if not isinstance(value, dict) or not isinstance(value.get("svg"), str):
            break
        serialized_svg = value["svg"]
        pass_applied = value.get("applied") if isinstance(value.get("applied"), list) else []
        if not pass_applied:
            break
        applied.extend(pass_applied)
        measured = measure_current_document(session, svg_path)
        after = classify_measurements(measured, require_markers=True)
        if after["passed"]:
            break
        current = after

    # A partial rewrite is discarded: mechanical repair may only commit a page
    # when every hard geometry error is gone and the visible text is unchanged.
    if not applied:
        return before, []
    if not after["passed"]:
        before["autoFixRejected"] = applied
        before["autoFixRemainingHardErrors"] = after.get("hardErrors", [])
        before["failureKind"] = "content_overflow"
        return before, []
    if measured is None or serialized_svg is None:
        return before, []
    after_visible_text = [item.get("text", "") for item in measured.get("texts", [])]
    if before_visible_text != after_visible_text:
        return before, []
    temp = svg_path.with_name(f".{svg_path.name}.{os.getpid()}.tmp")
    temp.write_text(serialized_svg, encoding="utf-8")
    os.replace(temp, svg_path)
    after["autoFixApplied"] = applied
    after["repairStatus"] = "repaired"
    return after, applied


def measure_current_document(session: BrowserSession, svg_path: Path) -> dict[str, Any]:
    evaluated = session.command(
        "Runtime.evaluate",
        {"expression": MEASURE_SCRIPT, "awaitPromise": True, "returnByValue": True},
    )
    result = evaluated.get("result", {})
    if result.get("subtype") == "error" or "value" not in result:
        raise RuntimeError(f"Chromium 无法复测 SVG: {result.get('description', result)}")
    measured = result["value"]
    measured["svgPath"] = str(svg_path.resolve())
    return measured


def bootstrap_v2_regions(session: BrowserSession, svg_path: Path) -> int:
    """Migrate a pre-v3 acceptance artifact; production never calls this path."""
    evaluated = session.command(
        "Runtime.evaluate",
        {"expression": BOOTSTRAP_REGION_SCRIPT, "awaitPromise": True, "returnByValue": True},
    )
    value = evaluated.get("result", {}).get("value")
    if not isinstance(value, dict) or not isinstance(value.get("svg"), str):
        raise RuntimeError("Chromium failed to bootstrap v2 text region metadata")
    temp = svg_path.with_name(f".{svg_path.name}.{os.getpid()}.tmp")
    temp.write_text(value["svg"], encoding="utf-8")
    os.replace(temp, svg_path)
    return int(value.get("count") or 0)


def run(
    svg_path: Path,
    require_markers: bool,
    auto_fix: bool = False,
    bootstrap_regions: bool = False,
) -> dict[str, Any]:
    if not svg_path.is_file():
        raise RuntimeError(f"SVG 文件不存在: {svg_path}")
    session = open_browser()
    try:
        measured = measure_svg(session, svg_path)
        if bootstrap_regions:
            bootstrap_count = bootstrap_v2_regions(session, svg_path)
            measured = measure_current_document(session, svg_path)
        else:
            bootstrap_count = 0
        before_visible_text = [item.get("text", "") for item in measured.get("texts", [])]
        report = classify_measurements(measured, require_markers=require_markers)
        if auto_fix and require_markers and not report["passed"]:
            report, _ = attempt_safe_fix(session, svg_path, report, before_visible_text)
            if not report["passed"]:
                report.setdefault("failureKind", "content_overflow")
        if bootstrap_count:
            report["bootstrappedRegionCount"] = bootstrap_count
        return report
    finally:
        session.close()


class GeometryClassificationTests(unittest.TestCase):
    def block(self, text: str, box: tuple[float, float, float, float], **overrides: Any) -> dict[str, Any]:
        result = {
            "elementId": text,
            "text": text,
            "bbox": dict(zip(("x", "y", "width", "height"), box)),
            "role": "body",
            "regionId": "region",
            "region": {"x": 100, "y": 100, "width": 300, "height": 120},
            "safePadding": 8,
            "maxLines": 3,
            "lineCount": 1,
        }
        result.update(overrides)
        return result

    def classify(self, *blocks: dict[str, Any]) -> dict[str, Any]:
        return classify_measurements({"svgPath": "fixture.svg", "texts": list(blocks), "obstacles": []})

    def test_metric_and_unit_vertical_overlap_is_hard(self) -> None:
        report = self.classify(
            self.block("3+", (120, 120, 80, 60), role="metric", regionId="metric"),
            self.block("核心理论体系", (130, 165, 110, 20), role="unit", regionId="unit"),
        )
        self.assertTrue(any(issue["rule"] == "text_text_overlap" for issue in report["hardErrors"]))

    def test_long_chinese_title_outside_card_is_hard(self) -> None:
        report = self.classify(self.block("很长的中文标题", (110, 120, 310, 24), role="title"))
        self.assertTrue(any(issue["rule"] == "text_outside_declared_region" for issue in report["hardErrors"]))

    def test_three_lines_exceeding_height_is_hard(self) -> None:
        report = self.classify(self.block("三行正文", (110, 150, 200, 90), lineCount=3))
        self.assertFalse(report["passed"])

    def test_middle_anchor_actual_bbox_drives_overflow(self) -> None:
        report = self.classify(self.block("居中文字", (80, 120, 150, 20), textAnchor="middle"))
        self.assertTrue(any(issue["rule"] == "text_outside_declared_region" for issue in report["hardErrors"]))

    def test_tspan_line_count_limit_is_hard(self) -> None:
        report = self.classify(self.block("多行", (110, 110, 120, 70), lineCount=4, maxLines=3))
        self.assertTrue(any(issue["rule"] == "text_exceeds_max_lines" for issue in report["hardErrors"]))

    def test_canvas_overflow_is_hard(self) -> None:
        report = self.classify(self.block("画布外", (1270, 100, 30, 20), region={"x": 1200, "y": 80, "width": 80, "height": 80}))
        self.assertTrue(any(issue["rule"] == "text_outside_canvas" for issue in report["hardErrors"]))

    def test_tight_padding_is_warning_only(self) -> None:
        report = self.classify(self.block("贴边", (102, 110, 80, 20)))
        self.assertTrue(report["passed"])
        self.assertTrue(any(issue["rule"] == "text_safe_padding_tight" for issue in report["warnings"]))

    def test_small_spacing_is_warning_not_hard(self) -> None:
        report = self.classify(
            self.block("上", (120, 120, 80, 20)),
            self.block("下", (120, 138.8, 80, 20), regionId="other"),
        )
        self.assertTrue(any(issue["rule"] == "text_text_spacing_tight" for issue in report["warnings"]))

    def test_obvious_overlap_is_hard(self) -> None:
        report = self.classify(
            self.block("上层", (120, 120, 100, 30)),
            self.block("下层", (120, 135, 100, 30), regionId="other"),
        )
        self.assertTrue(any(issue["rule"] == "text_text_overlap" for issue in report["hardErrors"]))

    def test_complex_free_layout_with_separate_blocks_passes(self) -> None:
        report = self.classify(
            self.block("自由标题", (112, 112, 120, 30), role="title", regionId="hero"),
            self.block("自由正文", (112, 180, 180, 30), regionId="body", region={"x": 100, "y": 160, "width": 300, "height": 100}),
        )
        self.assertTrue(report["passed"])

    def test_missing_markers_is_hard(self) -> None:
        block = self.block("无标记", (110, 110, 100, 20))
        block["region"] = None
        report = self.classify(block)
        self.assertTrue(any(issue["rule"] == "missing_text_region_metadata" for issue in report["hardErrors"]))

    def test_allow_overlap_suppresses_intentional_decoration(self) -> None:
        report = self.classify(
            self.block("装饰数字", (120, 120, 100, 40), allowOverlap=True),
            self.block("标签", (120, 130, 100, 20), regionId="label"),
        )
        self.assertFalse(any(issue["rule"] == "text_text_overlap" for issue in report["hardErrors"]))

    def test_obstacle_does_not_collide_with_its_own_descendant_text(self) -> None:
        owned = self.block(
            "node label",
            (120, 120, 100, 24),
            domIndex=0,
            regionId="node-1",
        )
        obstacle = {
            "elementId": "node-1",
            "regionId": "node-1",
            "bbox": {"x": 100, "y": 100, "width": 180, "height": 100},
            "containsTextDomIndexes": [0],
        }
        owned_report = classify_measurements(
            {"svgPath": "fixture.svg", "texts": [owned], "obstacles": [obstacle]}
        )
        self.assertFalse(
            any(issue["rule"] == "text_obstacle_overlap" for issue in owned_report["hardErrors"])
        )

        sibling_report = classify_measurements(
            {
                "svgPath": "fixture.svg",
                "texts": [dict(owned, domIndex=1)],
                "obstacles": [obstacle],
            }
        )
        self.assertTrue(
            any(issue["rule"] == "text_obstacle_overlap" for issue in sibling_report["hardErrors"])
        )

    def test_same_region_background_container_is_not_a_text_obstacle(self) -> None:
        block = self.block(
            "card body",
            (120, 120, 100, 24),
            domIndex=0,
            regionId="card-body",
        )
        background = {
            "elementId": "card-background",
            "regionId": "card-body",
            "bbox": {"x": 80, "y": 80, "width": 360, "height": 180},
            "containsTextDomIndexes": [],
        }
        background_report = classify_measurements(
            {"svgPath": "fixture.svg", "texts": [block], "obstacles": [background]}
        )
        self.assertFalse(
            any(
                issue["rule"] == "text_obstacle_overlap"
                for issue in background_report["hardErrors"]
            )
        )

        icon = dict(
            background,
            elementId="card-icon",
            bbox={"x": 130, "y": 110, "width": 40, "height": 40},
        )
        icon_report = classify_measurements(
            {"svgPath": "fixture.svg", "texts": [block], "obstacles": [icon]}
        )
        self.assertTrue(
            any(issue["rule"] == "text_obstacle_overlap" for issue in icon_report["hardErrors"])
        )

    def test_normal_numbers_and_dates_are_not_special_cased(self) -> None:
        report = self.classify(self.block("1893—1976 / 50亿", (112, 112, 180, 24)))
        self.assertTrue(report["passed"])


class GeometryBrowserFixTests(unittest.TestCase):
    def fixture(self, *, width: int, height: int, max_lines: int, min_font: int, text: str) -> tuple[tempfile.TemporaryDirectory[str], Path]:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-test-")
        path = Path(directory.name) / "fixture.svg"
        path.write_text(
            f'''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<rect x="100" y="100" width="{width}" height="{height}" fill="#eee"/>
<text id="body" x="110" y="130" font-family="Microsoft YaHei" font-size="20"
 data-pome-role="body" data-pome-region-id="card-body"
 data-pome-region-x="100" data-pome-region-y="100"
 data-pome-region-width="{width}" data-pome-region-height="{height}"
 data-pome-min-font-size="{min_font}" data-pome-wrap="true"
 data-pome-max-lines="{max_lines}" data-pome-safe-padding="10">{text}</text>
</svg>''',
            encoding="utf-8",
        )
        return directory, path

    def test_start_middle_end_anchors_and_transform_use_canvas_bounds(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-anchor-transform-test-")
        path = Path(directory.name) / "anchors.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text x="100" y="100" text-anchor="start" font-family="Arial" font-size="16" data-pome-role="label" data-pome-region-id="start" data-pome-region-x="90" data-pome-region-y="78" data-pome-region-width="120" data-pome-region-height="30" data-pome-min-font-size="14" data-pome-wrap="false" data-pome-max-lines="1" data-pome-line-height="19.2" data-pome-safe-padding="2">Start</text>
<text x="300" y="100" text-anchor="middle" font-family="Arial" font-size="16" data-pome-role="label" data-pome-region-id="middle" data-pome-region-x="240" data-pome-region-y="78" data-pome-region-width="120" data-pome-region-height="30" data-pome-min-font-size="14" data-pome-wrap="false" data-pome-max-lines="1" data-pome-line-height="19.2" data-pome-safe-padding="2">Middle</text>
<text x="500" y="100" text-anchor="end" font-family="Arial" font-size="16" data-pome-role="label" data-pome-region-id="end" data-pome-region-x="380" data-pome-region-y="78" data-pome-region-width="130" data-pome-region-height="30" data-pome-min-font-size="14" data-pome-wrap="false" data-pome-max-lines="1" data-pome-line-height="19.2" data-pome-safe-padding="2">End</text>
<text x="600" y="100" text-anchor="start" transform="translate(40 30)" font-family="Arial" font-size="16" data-pome-role="label" data-pome-region-id="translated" data-pome-region-x="630" data-pome-region-y="108" data-pome-region-width="120" data-pome-region-height="30" data-pome-min-font-size="14" data-pome-wrap="false" data-pome-max-lines="1" data-pome-line-height="19.2" data-pome-safe-padding="2">Shift</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=False)
            self.assertTrue(report["passed"], report)
            blocks = {item["regionId"]: item for item in report["textBlocks"]}
            self.assertEqual(blocks["start"]["textAnchor"], "start")
            self.assertEqual(blocks["middle"]["textAnchor"], "middle")
            self.assertEqual(blocks["end"]["textAnchor"], "end")
            self.assertEqual(blocks["translated"]["transform"], "translate(40 30)")
            self.assertAlmostEqual(blocks["translated"]["localToCanvas"]["e"], 40, delta=0.1)
            self.assertAlmostEqual(blocks["translated"]["localToCanvas"]["f"], 30, delta=0.1)
            self.assertGreaterEqual(blocks["translated"]["bbox"]["x"], 639)
        finally:
            directory.cleanup()

    def test_mixed_chinese_english_rich_tspans_keep_individual_sizes(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-rich-tspan-test-")
        path = Path(directory.name) / "rich.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text x="120" y="180" text-anchor="start" font-family="Microsoft YaHei, Arial" font-size="14" data-pome-role="body" data-pome-region-id="rich" data-pome-region-x="110" data-pome-region-y="145" data-pome-region-width="520" data-pome-region-height="60" data-pome-min-font-size="12" data-pome-wrap="false" data-pome-max-lines="1" data-pome-line-height="24" data-pome-safe-padding="4"><tspan font-size="14">中英文 mixed </tspan><tspan font-size="20" font-weight="700">重点 1976</tspan><tspan font-size="14"> 保持同一行</tspan></text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=False)
            self.assertTrue(report["passed"], report)
            block = report["textBlocks"][0]
            self.assertEqual(block["text"], "中英文 mixed 重点 1976 保持同一行")
            self.assertEqual([item["fontSize"] for item in block["tspans"]], [14, 20, 14])
        finally:
            directory.cleanup()

    def test_adjacent_compatible_rich_lines_merge_to_declared_limit(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-rich-line-merge-")
        path = Path(directory.name) / "rich-lines.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text text-anchor="middle" data-pome-role="body" data-pome-region-id="body"
 data-pome-region-x="150" data-pome-region-y="158" data-pome-region-width="100"
 data-pome-region-height="88" data-pome-min-font-size="13" data-pome-wrap="true"
 data-pome-max-lines="5" data-pome-line-height="17" data-pome-safe-padding="6">
 <tspan x="200" y="176" font-family="Microsoft YaHei" font-size="13" fill="#ffffff">六届六中</tspan>
 <tspan x="200" dy="17" font-family="Microsoft YaHei" font-size="13" fill="#ffffff">全会</tspan>
 <tspan x="200" dy="17" font-family="Microsoft YaHei" font-size="13" fill="#94a3b8">批判右倾</tspan>
 <tspan x="200" dy="17" font-family="Microsoft YaHei" font-size="13" fill="#94a3b8">投降主义</tspan>
 <tspan x="200" dy="17" font-family="Microsoft YaHei" font-size="13" fill="#94a3b8">确立自主</tspan>
 <tspan x="200" dy="17" font-family="Microsoft YaHei" font-size="13" fill="#94a3b8">方针</tspan>
</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before = run(path, require_markers=True, auto_fix=False)
            before_text = before["textBlocks"][0]["text"]
            self.assertTrue(
                any(
                    item["rule"] == "text_exceeds_max_lines"
                    for item in before["hardErrors"]
                )
            )
            after = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(after["passed"], after)
            self.assertEqual(before_text, after["textBlocks"][0]["text"])
            self.assertEqual(after["textBlocks"][0]["lineCount"], 5)
            self.assertTrue(
                any(
                    item.get("action") == "merge-adjacent-compatible-tspan-lines"
                    for item in after.get("autoFixApplied", [])
                )
            )
            fills = [item["fill"] for item in after["textBlocks"][0]["tspans"]]
            self.assertEqual(len(set(fills)), 2)
        finally:
            directory.cleanup()

    def test_missing_baseline_repair_preserves_text_and_avoids_new_collision(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-baseline-collision-test-")
        path = Path(directory.name) / "timeline.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text x="880" y="558" text-anchor="start" font-family="Arial" font-size="16" font-weight="700" data-pome-role="label" data-pome-region-id="year" data-pome-region-x="880" data-pome-region-y="542" data-pome-region-width="60" data-pome-region-height="22" data-pome-min-font-size="14" data-pome-wrap="false" data-pome-max-lines="1" data-pome-line-height="19.2" data-pome-safe-padding="2">1972</text>
<text text-anchor="start" data-pome-role="body" data-pome-region-id="body" data-pome-region-x="880" data-pome-region-y="530" data-pome-region-width="195" data-pome-region-height="60" data-pome-min-font-size="13" data-pome-wrap="true" data-pome-max-lines="3" data-pome-line-height="20" data-pome-safe-padding="4"><tspan x="880" dy="0" font-family="Microsoft YaHei" font-size="14">尼克松总统访华</tspan><tspan x="880" dy="20" font-family="Microsoft YaHei" font-size="14">打破长期隔绝</tspan><tspan x="880" dy="20" font-family="Microsoft YaHei" font-size="14">外交格局转折</tspan></text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before = run(path, require_markers=True, auto_fix=False)
            before_text = [item["text"] for item in before["textBlocks"]]
            self.assertTrue(any(item["rule"] == "missing_first_line_baseline" for item in before["hardErrors"]))
            after = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(after["passed"], after)
            self.assertEqual(before_text, [item["text"] for item in after["textBlocks"]])
            self.assertFalse(any(item["rule"] == "text_text_overlap" for item in after["hardErrors"]))
        finally:
            directory.cleanup()

    def test_safe_wrap_and_local_font_reduction_pass(self) -> None:
        directory, path = self.fixture(
            width=180,
            height=90,
            max_lines=3,
            min_font=16,
            text="这是一段需要在卡片内部安全换行的中文正文",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            self.assertTrue(report.get("autoFixApplied"))
            updated = path.read_text(encoding="utf-8")
            self.assertIn("<tspan", updated)
            self.assertIn("data-pome-min-font-size=\"16\"", updated)
        finally:
            directory.cleanup()

    def test_shallow_wrappable_body_promotes_to_two_lines_away_from_metric(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-two-line-body-test-")
        path = Path(directory.name) / "two-line-body.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body" x="184" y="425" font-family="Microsoft YaHei" font-size="16"
 data-pome-role="body" data-pome-region-id="card-body"
 data-pome-region-x="184" data-pome-region-y="402"
 data-pome-region-width="380" data-pome-region-height="24"
 data-pome-min-font-size="13" data-pome-wrap="true"
 data-pome-max-lines="1" data-pome-safe-padding="6">过渡时期总路线 · 三大改造 · 《五四宪法》 · 双百方针 · 《论十大关系》《实践论》</text>
<text id="metric" x="720" y="438" font-family="Georgia" font-size="36"
 data-pome-role="metric" data-pome-region-id="metric"
 data-pome-region-x="710" data-pome-region-y="402"
 data-pome-region-width="120" data-pome-region-height="48"
 data-pome-min-font-size="28" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="8">1953</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before = run(path, require_markers=True, auto_fix=False)
            self.assertTrue(
                any(item["rule"] == "text_text_overlap" for item in before["hardErrors"])
            )
            after = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(after["passed"], after)
            self.assertTrue(
                any(
                    item.get("action") == "promote-shallow-body-to-two-lines"
                    for item in (after.get("autoFixApplied") or [])
                )
            )
            updated = path.read_text(encoding="utf-8")
            self.assertIn('data-pome-max-lines="2"', updated)
            self.assertIn("<tspan", updated)
        finally:
            directory.cleanup()

    def test_shallow_body_is_not_promoted_when_second_line_leaves_canvas(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-two-line-canvas-test-")
        path = Path(directory.name) / "two-line-canvas.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body" x="184" y="705" font-family="Microsoft YaHei" font-size="16"
 data-pome-role="body" data-pome-region-id="card-body"
 data-pome-region-x="184" data-pome-region-y="680"
 data-pome-region-width="380" data-pome-region-height="24"
 data-pome-min-font-size="13" data-pome-wrap="true"
 data-pome-max-lines="1" data-pome-safe-padding="6">过渡时期总路线 · 三大改造 · 《五四宪法》 · 双百方针 · 《论十大关系》《实践论》</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertFalse(report["passed"])
            self.assertFalse(
                any(
                    item.get("action") == "promote-shallow-body-to-two-lines"
                    for item in (report.get("autoFixApplied") or [])
                )
            )
        finally:
            directory.cleanup()

    def test_minimum_font_still_cannot_fit_remains_hard(self) -> None:
        directory, path = self.fixture(
            width=80,
            height=32,
            max_lines=1,
            min_font=20,
            text="无法塞进单行区域的长中文正文",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertFalse(report["passed"])
            self.assertFalse(report.get("autoFixApplied"))
            self.assertEqual(report.get("failureKind"), "content_overflow")
        finally:
            directory.cleanup()

    def test_small_non_colliding_region_shortfall_expands_only_that_region(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-region-test-")
        path = Path(directory.name) / "footer.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="page" x="1224" y="685" text-anchor="end" font-family="Consolas" font-size="13"
 data-pome-role="footer" data-pome-region-id="page-number"
 data-pome-region-x="1180" data-pome-region-y="670"
 data-pome-region-width="44" data-pome-region-height="20"
 data-pome-min-font-size="12" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="8">01 / 06</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertTrue(
                any(
                    item.get("action")
                    in {"expand-region", "relocate-compact-region-before-text"}
                    for item in applied
                )
            )
            updated = path.read_text(encoding="utf-8")
            self.assertIn('data-pome-region-id="page-number"', updated)
            self.assertIn("01 / 06", updated)
        finally:
            directory.cleanup()

    def test_single_line_subtitle_can_expand_into_proven_empty_space(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-subtitle-width-")
        path = Path(directory.name) / "subtitle.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text x="64" y="128" text-anchor="start" font-family="Arial" font-size="16"
 data-pome-role="subtitle" data-pome-region-id="page-subtitle"
 data-pome-region-x="48" data-pome-region-y="100"
 data-pome-region-width="300" data-pome-region-height="36"
 data-pome-min-font-size="16" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="6">xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before_x = 'x="64"'
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            self.assertIn(before_x, path.read_text(encoding="utf-8"))
            self.assertTrue(
                any(
                    item.get("action") == "expand-region"
                    and item.get("expansion", 0) > 12
                    for item in (report.get("autoFixApplied") or [])
                )
            )
        finally:
            directory.cleanup()

    def test_relocated_end_anchor_region_grows_to_measured_footer_width(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-end-anchor-test-")
        path = Path(directory.name) / "footer.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body" x="100" y="220" font-family="Microsoft YaHei" font-size="15"
 data-pome-role="body" data-pome-region-id="body"
 data-pome-region-x="100" data-pome-region-y="200"
 data-pome-region-width="440" data-pome-region-height="40"
 data-pome-min-font-size="13" data-pome-wrap="true"
 data-pome-max-lines="2" data-pome-safe-padding="4">1950—1956年完成土地改革、抗美援朝保家卫国、社会主义改造与工业体系建设，并推进农业手工业和资本主义工商业改造</text>
<text id="page" x="1200" y="688" text-anchor="end" font-family="Consolas" font-size="14" letter-spacing="2"
 data-pome-role="footer" data-pome-region-id="page-number"
 data-pome-region-x="1200" data-pome-region-y="670"
 data-pome-region-width="50" data-pome-region-height="20"
 data-pome-min-font-size="12" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="2">03 / 06</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertTrue(any(item.get("lineCount") == 2 for item in applied))
            repair = next(
                item
                for item in applied
                if item.get("action")
                in {"relocate-region", "relocate-compact-region-before-text"}
            )
            self.assertGreater(repair["region"]["width"], 50)
            updated = path.read_text(encoding="utf-8")
            self.assertIn("<tspan", updated)
            self.assertIn("03 / 06", updated)
        finally:
            directory.cleanup()

    def test_end_anchored_metric_region_relocates_to_its_measured_bounds(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-metric-anchor-test-")
        path = Path(directory.name) / "metric.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="metric" x="365" y="290" text-anchor="end" font-family="Georgia" font-size="52"
 data-pome-role="metric" data-pome-region-id="card-metric"
 data-pome-region-x="340" data-pome-region-y="240"
 data-pome-region-width="50" data-pome-region-height="70"
 data-pome-min-font-size="52" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="4">01</text>
<text id="body" x="300" y="318" font-family="Arial" font-size="14"
 data-pome-role="body" data-pome-region-id="card-body"
 data-pome-region-x="290" data-pome-region-y="300"
 data-pome-region-width="200" data-pome-region-height="40"
 data-pome-min-font-size="12" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="4">Nearby body text</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            repair = next(
                item
                for item in (report.get("autoFixApplied") or [])
                if item.get("action")
                in {"relocate-region", "relocate-compact-region-before-text"}
            )
            self.assertGreater(repair["region"]["width"], 50)
            updated = path.read_text(encoding="utf-8")
            self.assertIn('x="365"', updated)
            self.assertIn(">01</text>", updated)
        finally:
            directory.cleanup()

    def test_isolated_decorative_glyph_gets_complete_region_metadata(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-glyph-test-")
        path = Path(directory.name) / "glyph.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="glyph" x="640" y="360" text-anchor="middle" font-family="Segoe UI Emoji" font-size="20">🤝</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertTrue(
                any(
                    item.get("action") == "declare-decorative-glyph-region"
                    for item in applied
                )
            )
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            glyph = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(glyph)
            if glyph is not None:
                self.assertEqual(glyph.get("data-pome-role"), "label")
                self.assertEqual(glyph.get("data-pome-wrap"), "false")
                self.assertEqual(glyph.get("data-pome-max-lines"), "1")
                for field in ("x", "y", "width", "height"):
                    self.assertIsNotNone(glyph.get(f"data-pome-region-{field}"))
        finally:
            directory.cleanup()

    def test_text_inside_marked_obstacle_gets_complete_region_metadata(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-container-text-test-")
        path = Path(directory.name) / "container-text.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<g id="node" data-pome-obstacle="true" data-pome-region-id="node-1921">
  <rect x="100" y="100" width="300" height="140" rx="16" fill="#eee"/>
  <text id="year" x="124" y="150" font-family="Arial" font-size="30" font-weight="700">1921</text>
  <text id="label" x="124" y="192" font-family="Arial" font-size="18">First congress</text>
</g>
</svg>''',
            encoding="utf-8",
        )
        try:
            before_root = ET.fromstring(path.read_text(encoding="utf-8"))
            before_text = [
                node.text
                for node in before_root.findall(".//{http://www.w3.org/2000/svg}text")
            ]
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertEqual(
                sum(
                    item.get("action") == "declare-container-text-region"
                    for item in applied
                ),
                2,
            )
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            texts = root.findall(".//{http://www.w3.org/2000/svg}text")
            self.assertEqual([node.text for node in texts], before_text)
            for node in texts:
                self.assertEqual(node.get("data-pome-region-id"), "node-1921")
                self.assertIsNotNone(node.get("data-pome-role"))
                self.assertEqual(node.get("data-pome-wrap"), "false")
                for field in ("x", "y", "width", "height"):
                    self.assertIsNotNone(node.get(f"data-pome-region-{field}"))
        finally:
            directory.cleanup()

    def test_text_over_same_region_card_background_is_not_an_obstacle_collision(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-card-background-test-")
        path = Path(directory.name) / "card-background.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<g id="card" data-pome-obstacle="true" data-pome-region-id="card-body">
  <rect x="100" y="100" width="340" height="260" rx="16" fill="#eee"/>
</g>
<text id="body" x="150" y="190" font-family="Arial" font-size="18"
 data-pome-role="body" data-pome-region-id="card-body"
 data-pome-region-x="140" data-pome-region-y="150"
 data-pome-region-width="250" data-pome-region-height="120"
 data-pome-min-font-size="16" data-pome-wrap="true"
 data-pome-max-lines="3" data-pome-safe-padding="8">Card body text</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            self.assertFalse(
                any(
                    issue.get("rule") == "text_obstacle_overlap"
                    for issue in report["hardErrors"]
                )
            )
        finally:
            directory.cleanup()

    def test_unmarked_normal_text_is_not_guessed_as_a_decorative_glyph(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-unmarked-text-test-")
        path = Path(directory.name) / "unmarked.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body" x="100" y="200" font-family="Arial" font-size="18">unmarked normal body text</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertFalse(report["passed"])
            self.assertTrue(
                any(
                    issue.get("rule") == "missing_text_region_metadata"
                    for issue in report["hardErrors"]
                )
            )
            self.assertFalse(report.get("autoFixApplied"))
        finally:
            directory.cleanup()

    def test_isolated_timeline_labels_get_narrow_region_metadata(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-timeline-label-test-")
        path = Path(directory.name) / "timeline-labels.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="year" x="102" y="646" text-anchor="middle" font-family="Consolas" font-size="13">1927</text>
<text id="event" x="102" y="666" text-anchor="middle" font-family="Microsoft YaHei" font-size="11">秋收起义</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertEqual(
                sum(
                    item.get("action") == "declare-isolated-label-region"
                    for item in applied
                ),
                2,
            )
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            texts = root.findall("{http://www.w3.org/2000/svg}text")
            self.assertEqual(texts[0].get("data-pome-role"), "label")
            self.assertEqual(texts[1].get("data-pome-role"), "caption")
            self.assertTrue(
                all(
                    (node.get("data-pome-region-id") or "").startswith(
                        "auto-isolated-label-"
                    )
                    for node in texts
                )
            )
        finally:
            directory.cleanup()

    def test_end_anchor_region_is_aligned_without_moving_visible_text(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-anchor-region-test-")
        path = Path(directory.name) / "anchor-region.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="range" x="1040" y="90" text-anchor="end" font-family="Consolas" font-size="16"
 data-pome-role="label" data-pome-region-id="range-label"
 data-pome-region-x="1040" data-pome-region-y="72"
 data-pome-region-width="200" data-pome-region-height="24"
 data-pome-min-font-size="14" data-pome-wrap="true"
 data-pome-max-lines="1" data-pome-safe-padding="4">1922 — 1945</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertTrue(
                any(
                    item.get("action") == "align-region-to-text-anchor"
                    for item in applied
                )
            )
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            text = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(text)
            if text is not None:
                self.assertEqual(text.get("x"), "1040")
                self.assertEqual(text.get("data-pome-region-x"), "840")
        finally:
            directory.cleanup()

    def test_middle_anchor_metric_region_is_aligned_without_moving_metric(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-middle-anchor-test-")
        path = Path(directory.name) / "middle-anchor.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="metric" x="760" y="278" text-anchor="middle" font-family="Georgia" font-size="36"
 data-pome-role="metric" data-pome-region-id="metric"
 data-pome-region-x="740" data-pome-region-y="242"
 data-pome-region-width="100" data-pome-region-height="48"
 data-pome-min-font-size="28" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="8">1949</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            self.assertTrue(
                any(
                    item.get("action") == "align-region-to-text-anchor"
                    for item in (report.get("autoFixApplied") or [])
                )
            )
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            text = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(text)
            if text is not None:
                self.assertEqual(text.get("x"), "760")
                self.assertEqual(text.get("data-pome-region-x"), "710")
        finally:
            directory.cleanup()

    def test_compact_start_label_relocates_region_before_visible_text(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-compact-label-test-")
        path = Path(directory.name) / "compact-label.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="period" x="55" y="218" font-family="Consolas" font-size="14"
 data-pome-role="label" data-pome-region-id="period-label"
 data-pome-region-x="20" data-pome-region-y="214"
 data-pome-region-width="80" data-pome-region-height="20"
 data-pome-min-font-size="12" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="4">1949—1952</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            self.assertTrue(
                any(
                    item.get("action") == "relocate-compact-region-before-text"
                    for item in (report.get("autoFixApplied") or [])
                )
            )
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            text = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(text)
            if text is not None:
                self.assertEqual(text.get("x"), "55")
                self.assertEqual(text.get("font-size"), "14")
        finally:
            directory.cleanup()

    def test_small_region_shortfall_can_expand_when_visible_text_does_not_collide(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-adjacent-region-test-")
        path = Path(directory.name) / "adjacent.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="first" x="880" y="578" text-anchor="middle" font-family="Microsoft YaHei" font-size="12"
 data-pome-role="body" data-pome-region-id="first"
 data-pome-region-x="770" data-pome-region-y="562"
 data-pome-region-width="220" data-pome-region-height="16"
 data-pome-min-font-size="11" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="0">尼克松访华</text>
<text id="second" x="880" y="594" text-anchor="middle" font-family="Microsoft YaHei" font-size="11"
 data-pome-role="body" data-pome-region-id="second"
 data-pome-region-x="770" data-pome-region-y="580"
 data-pome-region-width="220" data-pome-region-height="12"
 data-pome-min-font-size="11" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="0">《中美上海公报》</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            second_fix = next(
                item
                for item in applied
                if item.get("domIndex") == 1 and item.get("action") == "expand-region"
            )
            self.assertEqual(second_fix.get("action"), "expand-region")
            self.assertGreater(second_fix["region"]["height"], 12)
        finally:
            directory.cleanup()

    def test_non_colliding_footer_region_can_expand_without_moving_text(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-footer-test-")
        path = Path(directory.name) / "footer.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="footer" x="1160" y="670" text-anchor="end" font-family="Consolas" font-size="13" letter-spacing="2"
 data-pome-role="footer" data-pome-region-id="cover-footer"
 data-pome-region-x="960" data-pome-region-y="654"
 data-pome-region-width="200" data-pome-region-height="24"
 data-pome-min-font-size="11" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="6">INDUSTRIAL VISION INSPECTION</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before = path.read_text(encoding="utf-8")
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertTrue(
                any(
                    item.get("action")
                    in {"expand-region", "relocate-compact-region-before-text"}
                    for item in applied
                )
            )
            updated = path.read_text(encoding="utf-8")
            self.assertIn('x="1160" y="670"', updated)
            self.assertIn("INDUSTRIAL VISION INSPECTION", updated)
            self.assertNotEqual(before, updated)
        finally:
            directory.cleanup()

    def test_existing_plain_tspans_can_reflow_without_losing_text(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-tspan-test-")
        path = Path(directory.name) / "tspan.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body" x="100" y="300" font-family="Microsoft YaHei" font-size="18"
 data-pome-role="body" data-pome-region-id="card-body"
 data-pome-region-x="100" data-pome-region-y="266"
 data-pome-region-width="224" data-pome-region-height="80"
 data-pome-min-font-size="16" data-pome-wrap="true"
 data-pome-max-lines="3" data-pome-safe-padding="8">
 <tspan x="100" dy="0">人工目检难以在高速</tspan>
 <tspan x="100" dy="26">产线上稳定识别微小</tspan>
 <tspan x="100" dy="26">或复杂缺陷</tspan>
</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            updated = path.read_text(encoding="utf-8")
            self.assertIn("<tspan", updated)
            root = ET.fromstring(updated)
            text_element = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(text_element)
            visible = "".join(text_element.itertext()) if text_element is not None else ""
            self.assertEqual(
                "".join(visible.split()),
                "人工目检难以在高速产线上稳定识别微小或复杂缺陷",
            )
        finally:
            directory.cleanup()

    def test_tspans_without_a_base_position_are_moved_into_the_declared_region(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-unpositioned-tspan-test-")
        path = Path(directory.name) / "unpositioned-tspan.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body" font-family="Microsoft YaHei" font-size="13"
 data-pome-role="body" data-pome-region-id="card-note"
 data-pome-region-x="80" data-pome-region-y="475"
 data-pome-region-width="320" data-pome-region-height="55"
 data-pome-min-font-size="12" data-pome-wrap="true"
 data-pome-max-lines="3" data-pome-safe-padding="6">
 <tspan x="80" dy="0">Family members made sacrifices,</tspan>
 <tspan x="80" dy="20">while others survived.</tspan>
 <tspan x="80" dy="20">Their lives followed the revolution.</tspan>
</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            text_element = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(text_element)
            if text_element is not None:
                first_tspan = text_element.find("{http://www.w3.org/2000/svg}tspan")
                self.assertIsNotNone(first_tspan)
                if first_tspan is not None:
                    self.assertIsNotNone(first_tspan.get("y"))
                visible = "".join(text_element.itertext())
                self.assertEqual(
                    " ".join(visible.split()),
                    "Family members made sacrifices, while others survived. Their lives followed the revolution.",
                )
        finally:
            directory.cleanup()

    def test_uniform_styled_tspans_without_base_position_keep_style_and_move_into_region(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-styled-tspan-test-")
        path = Path(directory.name) / "styled-tspan.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body"
 data-pome-role="body" data-pome-region-id="timeline-body"
 data-pome-region-x="275" data-pome-region-y="200"
 data-pome-region-width="370" data-pome-region-height="44"
 data-pome-min-font-size="12" data-pome-wrap="true"
 data-pome-max-lines="2" data-pome-safe-padding="3">
 <tspan x="275" dy="20" font-family="Arial" font-size="14" fill="#52637a">Early study introduced new political ideas and public action.</tspan>
 <tspan x="275" dy="18" font-family="Arial" font-size="14" fill="#52637a">The movement then developed through sustained organization.</tspan>
</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            text_element = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(text_element)
            if text_element is not None:
                self.assertEqual(text_element.get("font-family"), "Arial")
                self.assertEqual(text_element.get("fill"), "#52637a")
                self.assertGreaterEqual(float(text_element.get("font-size", "0")), 12)
                first_tspan = text_element.find("{http://www.w3.org/2000/svg}tspan")
                self.assertIsNotNone(first_tspan)
                if first_tspan is not None:
                    self.assertIsNotNone(first_tspan.get("y"))
                    self.assertIsNone(first_tspan.get("fill"))
                visible = "".join(text_element.itertext())
                self.assertEqual(
                    " ".join(visible.split()),
                    "Early study introduced new political ideas and public action. "
                    "The movement then developed through sustained organization.",
                )
        finally:
            directory.cleanup()

    def test_mixed_tspan_emphasis_is_not_flattened_by_mechanical_fix(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-mixed-tspan-test-")
        path = Path(directory.name) / "mixed-tspan.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body"
 data-pome-role="body" data-pome-region-id="timeline-body"
 data-pome-region-x="275" data-pome-region-y="200"
 data-pome-region-width="370" data-pome-region-height="44"
 data-pome-min-font-size="12" data-pome-wrap="true"
 data-pome-max-lines="2" data-pome-safe-padding="3">
 <tspan x="275" dy="20" font-family="Arial" font-size="14" fill="#52637a">Normal text remains normal.</tspan>
 <tspan x="275" dy="18" font-family="Arial" font-size="14" fill="#2563eb">Emphasized text keeps its color.</tspan>
</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            text_element = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(text_element)
            if text_element is not None:
                tspans = text_element.findall("{http://www.w3.org/2000/svg}tspan")
                self.assertEqual([item.get("fill") for item in tspans], ["#52637a", "#2563eb"])
                self.assertIsNotNone(tspans[0].get("y"))
                self.assertEqual(
                    " ".join("".join(text_element.itertext()).split()),
                    "Normal text remains normal. Emphasized text keeps its color.",
                )
        finally:
            directory.cleanup()

    def test_mixed_tspan_region_drift_repairs_metadata_only(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-rich-region-test-")
        path = Path(directory.name) / "rich-region.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="body" text-anchor="start" font-family="Microsoft YaHei" font-size="16"
 data-pome-role="body" data-pome-region-id="card-body"
 data-pome-region-x="101" data-pome-region-y="200"
 data-pome-region-width="260" data-pome-region-height="60"
 data-pome-min-font-size="14" data-pome-wrap="true"
 data-pome-max-lines="2" data-pome-line-height="22" data-pome-safe-padding="0">
 <tspan x="100" y="220" font-size="14" fill="#52637a">普通文字保持原色</tspan>
 <tspan x="100" dy="22" font-size="14" fill="#2563eb">重点文字保持强调色</tspan>
</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before_root = ET.fromstring(path.read_text(encoding="utf-8"))
            before_text = before_root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(before_text)
            before_segments = [item.text for item in list(before_text or [])]
            before_styles = [item.get("fill") for item in list(before_text or [])]

            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            self.assertTrue(
                any(
                    item.get("action") == "expand-rich-text-region"
                    for item in (report.get("autoFixApplied") or [])
                )
            )

            after_root = ET.fromstring(path.read_text(encoding="utf-8"))
            after_text = after_root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(after_text)
            self.assertEqual([item.text for item in list(after_text or [])], before_segments)
            self.assertEqual([item.get("fill") for item in list(after_text or [])], before_styles)
            # Both authored runs explicitly render at 14px.  Hoisting their
            # identical size to the parent is appearance-preserving.
            self.assertEqual(after_text.get("font-size") if after_text is not None else None, "14")
        finally:
            directory.cleanup()

    def test_bounded_multipass_rechecks_title_after_malformed_tspan_block_moves(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-multipass-test-")
        path = Path(directory.name) / "multipass.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="title" x="80" y="62" font-family="Arial" font-size="40" font-weight="bold"
 data-pome-role="title" data-pome-region-id="page-title"
 data-pome-region-x="80" data-pome-region-y="28"
 data-pome-region-width="600" data-pome-region-height="56"
 data-pome-min-font-size="36" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="8">A page title</text>
<text id="body"
 data-pome-role="body" data-pome-region-id="timeline-body"
 data-pome-region-x="275" data-pome-region-y="200"
 data-pome-region-width="370" data-pome-region-height="44"
 data-pome-min-font-size="12" data-pome-wrap="true"
 data-pome-max-lines="2" data-pome-safe-padding="3">
 <tspan x="275" dy="20" font-family="Arial" font-size="14" fill="#52637a">The malformed body initially overlaps the title near the top.</tspan>
 <tspan x="275" dy="18" font-family="Arial" font-size="14" fill="#52637a">It belongs in the timeline region below.</tspan>
</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertGreaterEqual(len(applied), 2, applied)
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            title = root.find("{http://www.w3.org/2000/svg}text[@id='title']")
            body = root.find("{http://www.w3.org/2000/svg}text[@id='body']")
            self.assertIsNotNone(title)
            self.assertIsNotNone(body)
            if title is not None:
                self.assertEqual(" ".join("".join(title.itertext()).split()), "A page title")
            if body is not None:
                first_tspan = body.find("{http://www.w3.org/2000/svg}tspan")
                self.assertIsNotNone(first_tspan)
                if first_tspan is not None:
                    self.assertIsNotNone(first_tspan.get("y"))
        finally:
            directory.cleanup()

    def test_relocated_region_must_still_contain_the_text(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-relocation-fit-test-")
        path = Path(directory.name) / "relocation-fit.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text id="label" x="480" y="228" font-family="Consolas" font-size="14" letter-spacing="3"
 data-pome-role="label" data-pome-region-id="card-label"
 data-pome-region-x="480" data-pome-region-y="210"
 data-pome-region-width="120" data-pome-region-height="24"
 data-pome-min-font-size="14" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="4">// THOUGHT WORKS</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertFalse(
                any(item.get("action") == "relocate-region" for item in applied),
                applied,
            )
            root = ET.fromstring(path.read_text(encoding="utf-8"))
            label = root.find("{http://www.w3.org/2000/svg}text")
            self.assertIsNotNone(label)
            if label is not None:
                self.assertGreater(float(label.get("data-pome-region-width", "0")), 120)
        finally:
            directory.cleanup()

    def test_small_text_overlap_is_resolved_inside_its_own_region(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-overlap-test-")
        path = Path(directory.name) / "overlap.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text x="266" y="312" text-anchor="middle" font-family="Microsoft YaHei" font-size="18"
 data-pome-role="body" data-pome-region-id="node-body"
 data-pome-region-x="134" data-pome-region-y="286"
 data-pome-region-width="264" data-pome-region-height="44"
 data-pome-min-font-size="16" data-pome-wrap="true"
 data-pome-max-lines="2" data-pome-safe-padding="6">样本采集与标注</text>
<text x="266" y="330" text-anchor="middle" font-family="Consolas" font-size="17"
 data-pome-role="metric" data-pome-region-id="node-num"
 data-pome-region-x="250" data-pome-region-y="308"
 data-pome-region-width="32" data-pome-region-height="30"
 data-pome-min-font-size="14" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="4">1</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before = run(path, require_markers=True, auto_fix=False)
            self.assertFalse(before["passed"])
            self.assertTrue(any(item["rule"] == "text_text_overlap" for item in before["hardErrors"]))
            after = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(after["passed"], after)
            self.assertIn("样本采集与标注", path.read_text(encoding="utf-8"))
        finally:
            directory.cleanup()

    def test_wrong_region_metadata_relocates_to_safe_existing_text(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-relocate-test-")
        path = Path(directory.name) / "relocate.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text x="640" y="96" text-anchor="middle" font-family="Arial" font-size="14"
 data-pome-role="subtitle" data-pome-region-id="subtitle"
 data-pome-region-x="400" data-pome-region-y="78"
 data-pome-region-width="480" data-pome-region-height="24"
 data-pome-min-font-size="12" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="4">SUBTITLE</text>
<text x="640" y="140" text-anchor="middle" font-family="Microsoft YaHei" font-size="11"
 data-pome-role="label" data-pome-region-id="loop-label"
 data-pome-region-x="440" data-pome-region-y="85"
 data-pome-region-width="400" data-pome-region-height="20"
 data-pome-min-font-size="11" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="2">数据流方向：采集 → 计算 → 执行 → 追溯</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            applied = report.get("autoFixApplied") or []
            self.assertTrue(
                any(
                    item.get("action")
                    in {"relocate-region", "relocate-compact-region-before-text"}
                    for item in applied
                )
            )
        finally:
            directory.cleanup()

    def test_owned_single_line_subtitle_can_relocate_far_metadata_only(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-owned-subtitle-")
        path = Path(directory.name) / "owned-subtitle.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<g id="title-card"><rect x="56" y="28" width="1168" height="76" fill="#eef2ff"/></g>
<text x="76" y="78" text-anchor="start" font-family="Arial" font-size="36"
 data-pome-role="title" data-pome-region-id="page-title" data-pome-owner="title-card"
 data-pome-region-x="70" data-pome-region-y="36"
 data-pome-region-width="400" data-pome-region-height="64"
 data-pome-min-font-size="32" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="8">REVOLUTION ROAD</text>
<text x="520" y="78" text-anchor="start" font-family="Arial" font-size="20"
 data-pome-role="subtitle" data-pome-region-id="page-subtitle" data-pome-owner="title-card"
 data-pome-region-x="70" data-pome-region-y="36"
 data-pome-region-width="300" data-pome-region-height="30"
 data-pome-min-font-size="18" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="4">A half-century journey toward victory</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            before = run(path, require_markers=True, auto_fix=False)
            self.assertFalse(before["passed"])
            after = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(after["passed"], after)
            self.assertEqual(
                [item["text"] for item in after["textBlocks"]],
                [item["text"] for item in before["textBlocks"]],
            )
            self.assertTrue(
                any(
                    item.get("action") == "relocate-region"
                    and item.get("centerShift", 0) > 120
                    for item in (after.get("autoFixApplied") or [])
                )
            )
        finally:
            directory.cleanup()

    def test_region_extending_below_canvas_is_shifted_inside(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-canvas-region-test-")
        path = Path(directory.name) / "footer.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text x="56" y="718" font-family="Consolas" font-size="11"
 data-pome-role="footer" data-pome-region-id="footer-left"
 data-pome-region-x="56" data-pome-region-y="708"
 data-pome-region-width="300" data-pome-region-height="14"
 data-pome-min-font-size="11" data-pome-wrap="false"
 data-pome-max-lines="1" data-pome-safe-padding="2">工业机器人视觉检测系统</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            updated = path.read_text(encoding="utf-8")
            self.assertIn("工业机器人视觉检测系统", updated)
        finally:
            directory.cleanup()


    def test_authored_four_line_body_repairs_tight_max_line_contract(self) -> None:
        directory = tempfile.TemporaryDirectory(prefix="pome-geometry-line-contract-")
        path = Path(directory.name) / "four-lines.svg"
        path.write_text(
            '''<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="720" viewBox="0 0 1280 720">
<rect x="0" y="0" width="1280" height="720" fill="#fff"/>
<text x="100" y="130" font-family="Microsoft YaHei" font-size="16"
 data-pome-role="body" data-pome-region-id="body"
 data-pome-region-x="92" data-pome-region-y="112"
 data-pome-region-width="360" data-pome-region-height="70"
 data-pome-min-font-size="14" data-pome-wrap="true"
 data-pome-max-lines="3" data-pome-line-height="20" data-pome-safe-padding="4">
 <tspan x="100" y="130">第一行事实文字</tspan>
 <tspan x="100" dy="20">第二行事实文字</tspan>
 <tspan x="100" dy="20">第三行事实文字</tspan>
 <tspan x="100" dy="20">第四行事实文字</tspan>
</text>
</svg>''',
            encoding="utf-8",
        )
        try:
            original_text = "第一行事实文字第二行事实文字第三行事实文字第四行事实文字"
            report = run(path, require_markers=True, auto_fix=True)
            self.assertTrue(report["passed"], report)
            updated = path.read_text(encoding="utf-8")
            self.assertIn('data-pome-max-lines="4"', updated)
            self.assertEqual(
                "".join(item["text"] for item in report["textBlocks"]).replace(" ", ""),
                original_text,
            )
            self.assertTrue(
                any(
                    item.get("action") == "correct-authored-line-count-contract"
                    for item in report.get("autoFixApplied", [])
                )
            )
        finally:
            directory.cleanup()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--svg", type=Path)
    parser.add_argument("--allow-missing-markers", action="store_true")
    parser.add_argument("--auto-fix", action="store_true")
    parser.add_argument("--bootstrap-v2-regions", action="store_true")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.TestSuite(
            [
                unittest.defaultTestLoader.loadTestsFromTestCase(GeometryClassificationTests),
                unittest.defaultTestLoader.loadTestsFromTestCase(GeometryBrowserFixTests),
            ]
        )
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        return 0 if result.wasSuccessful() else 1
    if args.svg is None:
        parser.error("--svg is required unless --self-test is used")
    try:
        report = run(
            args.svg,
            require_markers=not args.allow_missing_markers,
            auto_fix=args.auto_fix,
            bootstrap_regions=args.bootstrap_v2_regions,
        )
        serialized = json.dumps(report, ensure_ascii=False)
        if args.report is not None:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            temp_report = args.report.with_name(f".{args.report.name}.{os.getpid()}.tmp")
            temp_report.write_text(serialized + "\n", encoding="utf-8")
            os.replace(temp_report, args.report)
        print(serialized)
        return 0 if report["passed"] else 2
    except Exception as error:  # Keep the Rust caller's failure actionable.
        print(json.dumps({"schemaVersion": 1, "passed": False, "checkerError": str(error)}, ensure_ascii=False))
        return 3


if __name__ == "__main__":
    sys.exit(main())
