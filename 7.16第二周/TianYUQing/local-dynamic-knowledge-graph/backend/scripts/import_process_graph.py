"""Replace the demo database with the mechanical manufacturing process graph."""

from __future__ import annotations

import argparse
import asyncio
import csv
import io
import json
import re
import zipfile
from collections import Counter
from pathlib import Path

from app.context_graph_client import close_neo4j, connect_neo4j, execute_cypher

ALLOWED_RELATIONSHIPS = {"HAS_SECTION", "CONTAINS", "HAS_CONCEPT", "RELATED_TO"}
CHAPTER_ORDER = {"第一章": 1, "第二章": 2, "第三章": 3, "第四章": 4, "第五章": 5, "第六章": 6, "第七章": 7}


def read_csv(archive: zipfile.ZipFile, name: str) -> list[dict[str, str]]:
    text = archive.read(name).decode("utf-8-sig")
    return list(csv.DictReader(io.StringIO(text)))


def clean_chapter_name(name: str) -> str:
    return re.sub(r"\s+", " ", name.replace("_", " ")).strip()


async def import_graph(zip_path: Path) -> None:
    with zipfile.ZipFile(zip_path) as archive:
        chapters = read_csv(archive, "chapters.csv")
        sections = read_csv(archive, "sections.csv")
        knowledges = read_csv(archive, "knowledges.csv")
        concepts = read_csv(archive, "concepts.csv")
        relationships = read_csv(archive, "relations.csv")

    for row in chapters:
        row["name"] = clean_chapter_name(row["name"])
        row["order"] = next((order for prefix, order in CHAPTER_ORDER.items() if row["name"].startswith(prefix)), 99)

    section_order: Counter[str] = Counter()
    for row in sections:
        section_order[row["chapter_id"]] += 1
        row["order"] = section_order[row["chapter_id"]]

    known_ids = {row["id"] for group in (chapters, sections, knowledges, concepts) for row in group}
    valid_relationships = [
        row for row in relationships
        if row["type"] in ALLOWED_RELATIONSHIPS and row["source"] in known_ids and row["target"] in known_ids
    ]
    skipped = len(relationships) - len(valid_relationships)

    await connect_neo4j()
    try:
        await execute_cypher("MATCH (n) DETACH DELETE n", collect=False)
        # The generated manufacturing demo defines label-specific uniqueness
        # constraints (for example Section.name). They are incompatible with
        # textbook sections such as "第一节 概述" appearing in several chapters.
        old_constraints = await execute_cypher("SHOW CONSTRAINTS YIELD name RETURN name", collect=False)
        for item in old_constraints:
            safe_name = item["name"].replace("`", "``")
            await execute_cypher(f"DROP CONSTRAINT `{safe_name}` IF EXISTS", collect=False)
        await execute_cypher("CREATE CONSTRAINT entity_id_unique IF NOT EXISTS FOR (n:Entity) REQUIRE n.id IS UNIQUE", collect=False)
        await execute_cypher("CREATE INDEX entity_name IF NOT EXISTS FOR (n:Entity) ON (n.name)", collect=False)

        await execute_cypher(
            "UNWIND $rows AS row CREATE (:Entity:Chapter {id: row.id, name: row.name, chapter_order: row.order})",
            {"rows": chapters}, collect=False,
        )
        await execute_cypher(
            "UNWIND $rows AS row CREATE (:Entity:Section {id: row.id, name: row.name, chapter_id: row.chapter_id, section_order: row.order})",
            {"rows": sections}, collect=False,
        )
        await execute_cypher(
            """UNWIND $rows AS row CREATE (:Entity:Knowledge {
                 id: row.id, name: row.name, content: row.content,
                 knowledge_type: row.knowledge_type, section_id: row.section_id
               })""",
            {"rows": knowledges}, collect=False,
        )
        await execute_cypher(
            "UNWIND $rows AS row CREATE (:Entity:Concept {id: row.id, name: row.name})",
            {"rows": concepts}, collect=False,
        )

        for rel_type in sorted(ALLOWED_RELATIONSHIPS):
            rows = [row for row in valid_relationships if row["type"] == rel_type]
            await execute_cypher(
                f"UNWIND $rows AS row MATCH (s:Entity {{id: row.source}}), (t:Entity {{id: row.target}}) MERGE (s)-[:{rel_type}]->(t)",
                {"rows": rows}, collect=False,
            )

        counts = await execute_cypher(
            "MATCH (n) WITH labels(n) AS labels, count(*) AS count RETURN labels, count ORDER BY labels[1]",
            collect=False,
        )
        print(json.dumps({"nodes": counts, "relationships": len(valid_relationships), "skipped": skipped}, ensure_ascii=False, indent=2))
    finally:
        await close_neo4j()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("zip_path", type=Path, nargs="?", default=Path(__file__).parents[2] / "data" / "机械制造工艺知识图谱_Neo4j导入包.zip")
    args = parser.parse_args()
    asyncio.run(import_graph(args.zip_path.resolve()))


if __name__ == "__main__":
    main()
