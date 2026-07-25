"""Safely import the mechanical manufacturing process graph from the course ZIP."""

from __future__ import annotations

import argparse
import asyncio
import csv
import io
import json
import os
import re
import zipfile
from collections import Counter
from pathlib import Path

from app.context_graph_client import close_neo4j, connect_neo4j, execute_cypher

ALLOWED_RELATIONSHIPS = {"HAS_SECTION", "CONTAINS", "HAS_CONCEPT", "RELATED_TO"}
CHAPTER_ORDER = {
    "第一章": 1,
    "第二章": 2,
    "第三章": 3,
    "第四章": 4,
    "第五章": 5,
    "第六章": 6,
    "第七章": 7,
}


def read_csv(archive: zipfile.ZipFile, name: str) -> list[dict[str, str]]:
    text = archive.read(name).decode("utf-8-sig")
    return list(csv.DictReader(io.StringIO(text)))


def clean_chapter_name(name: str) -> str:
    return re.sub(r"\s+", " ", name.replace("_", " ")).strip()


def default_zip_path() -> Path:
    data_dir = Path(__file__).resolve().parents[2] / "data"
    matches = sorted(data_dir.glob("*Neo4j*.zip"))
    if not matches:
        raise FileNotFoundError(f"No Neo4j import ZIP found in {data_dir}")
    return matches[0]


def validate_import_target(
    database: str,
    confirm_reset: bool,
    allow_default_database: bool,
) -> None:
    if not confirm_reset:
        raise RuntimeError(
            "Refusing to import: pass --confirm-reset to acknowledge that the target database will be cleared."
        )
    if database in {"", "system"}:
        raise RuntimeError("Refusing to import into an empty or system Neo4j database name.")
    if database == "neo4j" and not allow_default_database:
        raise RuntimeError(
            "Refusing to import into the default 'neo4j' database. "
            "Use a dedicated database name, or pass --allow-default-database only for an isolated test/dev Neo4j instance."
        )


async def import_graph(
    zip_path: Path,
    *,
    database: str,
    confirm_reset: bool,
    allow_default_database: bool = False,
) -> None:
    validate_import_target(database, confirm_reset, allow_default_database)
    os.environ["NEO4J_DATABASE"] = database

    with zipfile.ZipFile(zip_path) as archive:
        chapters = read_csv(archive, "chapters.csv")
        sections = read_csv(archive, "sections.csv")
        knowledges = read_csv(archive, "knowledges.csv")
        concepts = read_csv(archive, "concepts.csv")
        relationships = read_csv(archive, "relations.csv")

    for row in chapters:
        row["name"] = clean_chapter_name(row["name"])
        row["order"] = next(
            (order for prefix, order in CHAPTER_ORDER.items() if row["name"].startswith(prefix)),
            99,
        )

    section_order: Counter[str] = Counter()
    for row in sections:
        section_order[row["chapter_id"]] += 1
        row["order"] = section_order[row["chapter_id"]]

    known_ids = {
        row["id"]
        for group in (chapters, sections, knowledges, concepts)
        for row in group
    }
    valid_relationships = [
        row
        for row in relationships
        if row["type"] in ALLOWED_RELATIONSHIPS
        and row["source"] in known_ids
        and row["target"] in known_ids
    ]
    skipped = len(relationships) - len(valid_relationships)

    await connect_neo4j()
    try:
        await execute_cypher("MATCH (n) DETACH DELETE n", collect=False)
        old_constraints = await execute_cypher(
            "SHOW CONSTRAINTS YIELD name RETURN name",
            collect=False,
        )
        for item in old_constraints:
            safe_name = item["name"].replace("`", "``")
            await execute_cypher(f"DROP CONSTRAINT `{safe_name}` IF EXISTS", collect=False)
        await execute_cypher(
            "CREATE CONSTRAINT entity_id_unique IF NOT EXISTS FOR (n:Entity) REQUIRE n.id IS UNIQUE",
            collect=False,
        )
        await execute_cypher(
            "CREATE INDEX entity_name IF NOT EXISTS FOR (n:Entity) ON (n.name)",
            collect=False,
        )

        await execute_cypher(
            "UNWIND $rows AS row CREATE (:Entity:Chapter {id: row.id, name: row.name, chapter_order: row.order})",
            {"rows": chapters},
            collect=False,
        )
        await execute_cypher(
            "UNWIND $rows AS row CREATE (:Entity:Section {id: row.id, name: row.name, chapter_id: row.chapter_id, section_order: row.order})",
            {"rows": sections},
            collect=False,
        )
        await execute_cypher(
            """UNWIND $rows AS row CREATE (:Entity:Knowledge {
                 id: row.id, name: row.name, content: row.content,
                 knowledge_type: row.knowledge_type, section_id: row.section_id
               })""",
            {"rows": knowledges},
            collect=False,
        )
        await execute_cypher(
            "UNWIND $rows AS row CREATE (:Entity:Concept {id: row.id, name: row.name})",
            {"rows": concepts},
            collect=False,
        )

        for rel_type in sorted(ALLOWED_RELATIONSHIPS):
            rows = [row for row in valid_relationships if row["type"] == rel_type]
            await execute_cypher(
                f"UNWIND $rows AS row MATCH (s:Entity {{id: row.source}}), (t:Entity {{id: row.target}}) MERGE (s)-[:{rel_type}]->(t)",
                {"rows": rows},
                collect=False,
            )

        counts = await execute_cypher(
            "MATCH (n) WITH labels(n) AS labels, count(*) AS count RETURN labels, count ORDER BY labels[1]",
            collect=False,
        )
        print(
            json.dumps(
                {
                    "database": database,
                    "nodes": counts,
                    "relationships": len(valid_relationships),
                    "skipped": skipped,
                },
                ensure_ascii=False,
                indent=2,
            )
        )
    finally:
        await close_neo4j()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("zip_path", type=Path, nargs="?", default=default_zip_path())
    parser.add_argument("--database", required=True, help="Explicit isolated Neo4j database to replace.")
    parser.add_argument("--confirm-reset", action="store_true", help="Required: clear and replace the target graph database.")
    parser.add_argument(
        "--allow-default-database",
        action="store_true",
        help="Allow database=neo4j only when using an isolated test/dev Neo4j instance.",
    )
    args = parser.parse_args()
    asyncio.run(
        import_graph(
            args.zip_path.resolve(),
            database=args.database,
            confirm_reset=args.confirm_reset,
            allow_default_database=args.allow_default_database,
        )
    )


if __name__ == "__main__":
    main()
