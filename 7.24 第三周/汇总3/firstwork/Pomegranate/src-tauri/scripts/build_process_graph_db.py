"""Build the bundled mechanical process course graph SQLite database.

This script is a development/build tool. It reads the original course ZIP
export directly and never requires Docker, Neo4j, Java, WSL, or FastAPI.
The target database is generated in a temporary file first, validated, and
then atomically replaces the previous resource database.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import io
import json
import os
import re
import sqlite3
import sys
import zipfile
from collections import Counter
from pathlib import Path
from typing import Iterable


ALLOWED_RELATIONSHIPS = {"HAS_SECTION", "CONTAINS", "HAS_CONCEPT", "RELATED_TO"}
DATA_VERSION = "mechanical-process-graph-v1"
EXPECTED = {
    "chapters": 7,
    "sections": 47,
    "knowledges": 283,
    "concepts": 1449,
    "valid_relationships": 2389,
}


def script_root() -> Path:
    return Path(__file__).resolve().parent


def firstwork_root() -> Path:
    # Pomegranate/src-tauri/scripts -> firstwork
    return script_root().parents[2]


def default_zip_path() -> Path:
    data_dir = firstwork_root() / "mechanical-knowledge-graph-service" / "data"
    matches = sorted(data_dir.glob("*Neo4j*.zip"))
    if not matches:
        raise FileNotFoundError(f"No Neo4j import ZIP found in {data_dir}")
    return matches[0]


def default_output_path() -> Path:
    return script_root().parents[0] / "resources" / "process_graph.db"


def read_csv(archive: zipfile.ZipFile, name: str) -> list[dict[str, str]]:
    text = archive.read(name).decode("utf-8-sig")
    return list(csv.DictReader(io.StringIO(text)))


def clean_chapter_name(name: str) -> str:
    return re.sub(r"\s+", " ", name.replace("_", " ")).strip()


def chapter_order(name: str, fallback: int) -> int:
    match = re.match(r"第([一二三四五六七八九十]+)章", name)
    if not match:
        return fallback
    mapping = {
        "一": 1,
        "二": 2,
        "三": 3,
        "四": 4,
        "五": 5,
        "六": 6,
        "七": 7,
        "八": 8,
        "九": 9,
        "十": 10,
    }
    return mapping.get(match.group(1), fallback)


def load_source(zip_path: Path) -> dict[str, list[dict[str, str]]]:
    with zipfile.ZipFile(zip_path) as archive:
        tables = {
            "chapters": read_csv(archive, "chapters.csv"),
            "sections": read_csv(archive, "sections.csv"),
            "knowledges": read_csv(archive, "knowledges.csv"),
            "concepts": read_csv(archive, "concepts.csv"),
            "relationships": read_csv(archive, "relations.csv"),
        }
    for idx, row in enumerate(tables["chapters"], start=1):
        row["name"] = clean_chapter_name(row["name"])
        row["order"] = str(chapter_order(row["name"], idx))

    section_order: Counter[str] = Counter()
    for row in tables["sections"]:
        section_order[row["chapter_id"]] += 1
        row["order"] = str(section_order[row["chapter_id"]])
    return tables


def ensure_unique_ids(tables: dict[str, list[dict[str, str]]]) -> None:
    ids = [
        row["id"]
        for table in ("chapters", "sections", "knowledges", "concepts")
        for row in tables[table]
    ]
    duplicates = [node_id for node_id, count in Counter(ids).items() if count > 1]
    if duplicates:
        raise RuntimeError(f"Duplicate node ids found: {duplicates[:20]}")


def classify_relationships(
    tables: dict[str, list[dict[str, str]]],
) -> tuple[list[dict[str, str]], list[dict[str, str]]]:
    known_ids = {
        row["id"]
        for table in ("chapters", "sections", "knowledges", "concepts")
        for row in tables[table]
    }
    valid: list[dict[str, str]] = []
    invalid: list[dict[str, str]] = []
    for row in tables["relationships"]:
        if (
            row.get("type") in ALLOWED_RELATIONSHIPS
            and row.get("source") in known_ids
            and row.get("target") in known_ids
        ):
            valid.append(row)
        else:
            invalid.append(row)
    illegal_types = sorted({row.get("type", "") for row in invalid if row.get("type") not in ALLOWED_RELATIONSHIPS})
    if illegal_types:
        raise RuntimeError(f"Illegal relationship types found: {illegal_types}")
    return valid, invalid


def init_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(
        """
        PRAGMA journal_mode = OFF;
        PRAGMA synchronous = OFF;
        PRAGMA foreign_keys = ON;

        CREATE TABLE metadata (
          key TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );

        CREATE TABLE nodes (
          id TEXT PRIMARY KEY,
          node_type TEXT NOT NULL CHECK (node_type IN ('Chapter','Section','Knowledge','Concept')),
          name TEXT NOT NULL,
          content TEXT NOT NULL DEFAULT '',
          chapter_id TEXT,
          section_id TEXT,
          metadata TEXT NOT NULL DEFAULT '{}'
        );

        CREATE TABLE edges (
          id TEXT PRIMARY KEY,
          source_id TEXT NOT NULL,
          target_id TEXT NOT NULL,
          relation_type TEXT NOT NULL CHECK (relation_type IN ('HAS_SECTION','CONTAINS','HAS_CONCEPT','RELATED_TO')),
          metadata TEXT NOT NULL DEFAULT '{}',
          FOREIGN KEY (source_id) REFERENCES nodes(id),
          FOREIGN KEY (target_id) REFERENCES nodes(id)
        );

        CREATE TABLE chapters (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          chapter_order INTEGER NOT NULL,
          FOREIGN KEY (id) REFERENCES nodes(id)
        );

        CREATE TABLE sections (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          chapter_id TEXT NOT NULL,
          section_order INTEGER NOT NULL,
          FOREIGN KEY (id) REFERENCES nodes(id),
          FOREIGN KEY (chapter_id) REFERENCES chapters(id)
        );

        CREATE INDEX idx_nodes_type ON nodes(node_type);
        CREATE INDEX idx_nodes_name ON nodes(name);
        CREATE INDEX idx_nodes_chapter ON nodes(chapter_id);
        CREATE INDEX idx_nodes_section ON nodes(section_id);
        CREATE INDEX idx_edges_source ON edges(source_id);
        CREATE INDEX idx_edges_target ON edges(target_id);
        CREATE INDEX idx_edges_relation ON edges(relation_type);
        CREATE INDEX idx_edges_source_relation ON edges(source_id, relation_type);
        """
    )


def insert_node(
    conn: sqlite3.Connection,
    *,
    node_id: str,
    node_type: str,
    name: str,
    content: str = "",
    chapter_id: str | None = None,
    section_id: str | None = None,
    metadata: dict | None = None,
) -> None:
    conn.execute(
        """
        INSERT INTO nodes (id, node_type, name, content, chapter_id, section_id, metadata)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        """,
        (
            node_id,
            node_type,
            name,
            content or "",
            chapter_id,
            section_id,
            json.dumps(metadata or {}, ensure_ascii=False, sort_keys=True),
        ),
    )


def build_database(
    zip_path: Path,
    output_path: Path,
    *,
    replace: bool = True,
) -> dict[str, object]:
    tables = load_source(zip_path)
    ensure_unique_ids(tables)
    valid_relationships, invalid_relationships = classify_relationships(tables)

    output_path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = output_path.with_name(f".{output_path.name}.{os.getpid()}.tmp")
    if tmp_path.exists():
        tmp_path.unlink()

    section_by_id = {row["id"]: row for row in tables["sections"]}

    conn = sqlite3.connect(tmp_path)
    try:
        init_schema(conn)
        with conn:
            for row in tables["chapters"]:
                order = int(row["order"])
                insert_node(
                    conn,
                    node_id=row["id"],
                    node_type="Chapter",
                    name=row["name"],
                    chapter_id=row["id"],
                    metadata={"chapterOrder": order},
                )
                conn.execute(
                    "INSERT INTO chapters (id, name, chapter_order) VALUES (?1, ?2, ?3)",
                    (row["id"], row["name"], order),
                )

            for row in tables["sections"]:
                order = int(row["order"])
                insert_node(
                    conn,
                    node_id=row["id"],
                    node_type="Section",
                    name=row["name"],
                    chapter_id=row["chapter_id"],
                    metadata={"sectionOrder": order},
                )
                conn.execute(
                    "INSERT INTO sections (id, name, chapter_id, section_order) VALUES (?1, ?2, ?3, ?4)",
                    (row["id"], row["name"], row["chapter_id"], order),
                )

            for row in tables["knowledges"]:
                section = section_by_id.get(row["section_id"], {})
                insert_node(
                    conn,
                    node_id=row["id"],
                    node_type="Knowledge",
                    name=row["name"],
                    content=row.get("content", ""),
                    chapter_id=section.get("chapter_id"),
                    section_id=row["section_id"],
                    metadata={"knowledgeType": row.get("knowledge_type", "")},
                )

            for row in tables["concepts"]:
                insert_node(
                    conn,
                    node_id=row["id"],
                    node_type="Concept",
                    name=row["name"],
                )

            for index, row in enumerate(valid_relationships, start=1):
                conn.execute(
                    """
                    INSERT INTO edges (id, source_id, target_id, relation_type, metadata)
                    VALUES (?1, ?2, ?3, ?4, ?5)
                    """,
                    (
                        f"E{index:06d}",
                        row["source"],
                        row["target"],
                        row["type"],
                        "{}",
                    ),
                )

            stats = {
                "version": DATA_VERSION,
                "sourceZip": zip_path.name,
                "generatedAt": dt.datetime.now(dt.timezone.utc).isoformat(),
                "chapters": len(tables["chapters"]),
                "sections": len(tables["sections"]),
                "knowledges": len(tables["knowledges"]),
                "concepts": len(tables["concepts"]),
                "nodes": sum(len(tables[name]) for name in ("chapters", "sections", "knowledges", "concepts")),
                "edges": len(valid_relationships),
                "sourceRelationships": len(tables["relationships"]),
                "skippedInvalidRelationships": len(invalid_relationships),
                "invalidRelationships": invalid_relationships,
            }
            for key, value in stats.items():
                conn.execute(
                    "INSERT INTO metadata (key, value) VALUES (?1, ?2)",
                    (key, json.dumps(value, ensure_ascii=False) if isinstance(value, (list, dict)) else str(value)),
                )
        validate_database(conn, stats)
    except Exception:
        conn.close()
        tmp_path.unlink(missing_ok=True)
        raise
    else:
        conn.close()

    if output_path.exists() and not replace:
        tmp_path.unlink(missing_ok=True)
        raise FileExistsError(output_path)
    os.replace(tmp_path, output_path)
    return stats


def scalar(conn: sqlite3.Connection, sql: str, params: Iterable[object] = ()) -> int:
    return int(conn.execute(sql, tuple(params)).fetchone()[0])


def validate_database(conn: sqlite3.Connection, stats: dict[str, object]) -> None:
    checks = {
        "chapters": scalar(conn, "SELECT count(*) FROM nodes WHERE node_type = 'Chapter'"),
        "sections": scalar(conn, "SELECT count(*) FROM nodes WHERE node_type = 'Section'"),
        "knowledges": scalar(conn, "SELECT count(*) FROM nodes WHERE node_type = 'Knowledge'"),
        "concepts": scalar(conn, "SELECT count(*) FROM nodes WHERE node_type = 'Concept'"),
        "valid_relationships": scalar(conn, "SELECT count(*) FROM edges"),
    }
    for key, expected in EXPECTED.items():
        if checks[key] != expected:
            raise RuntimeError(f"Unexpected {key}: got {checks[key]}, expected {expected}")

    missing = scalar(
        conn,
        """
        SELECT count(*)
        FROM edges e
        LEFT JOIN nodes s ON s.id = e.source_id
        LEFT JOIN nodes t ON t.id = e.target_id
        WHERE s.id IS NULL OR t.id IS NULL
        """,
    )
    if missing:
        raise RuntimeError(f"Relationship endpoints missing after import: {missing}")

    if scalar(conn, "SELECT count(*) FROM edges WHERE relation_type NOT IN ('HAS_SECTION','CONTAINS','HAS_CONCEPT','RELATED_TO')"):
        raise RuntimeError("Illegal relationship type found after import")

    # Ensure the four UI-critical queries have data.
    if scalar(conn, "SELECT count(*) FROM chapters") != EXPECTED["chapters"]:
        raise RuntimeError("Chapter table validation failed")
    if scalar(conn, "SELECT count(*) FROM edges WHERE relation_type = 'HAS_SECTION'") != EXPECTED["sections"]:
        raise RuntimeError("Chapter -> Section relation validation failed")
    if scalar(conn, "SELECT count(*) FROM edges WHERE relation_type = 'CONTAINS'") != EXPECTED["knowledges"]:
        raise RuntimeError("Section -> Knowledge relation validation failed")
    if scalar(conn, "SELECT count(*) FROM edges WHERE relation_type = 'HAS_CONCEPT'") <= 0:
        raise RuntimeError("Knowledge -> Concept relation validation failed")
    if scalar(conn, "SELECT count(*) FROM edges WHERE relation_type = 'RELATED_TO'") <= 0:
        raise RuntimeError("RELATED_TO relation validation failed")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-zip", type=Path, default=default_zip_path())
    parser.add_argument("--output", type=Path, default=default_output_path())
    parser.add_argument("--no-replace", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    stats = build_database(
        args.source_zip.resolve(),
        args.output.resolve(),
        replace=not args.no_replace,
    )
    print(json.dumps(stats, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        raise
