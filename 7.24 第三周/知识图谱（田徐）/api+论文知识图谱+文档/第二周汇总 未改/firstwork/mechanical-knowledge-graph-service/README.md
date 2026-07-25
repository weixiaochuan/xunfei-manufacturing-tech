# Mechanical Manufacturing Process Knowledge Graph

This is a local FastAPI + Neo4j service for the course **机械制造工艺**.
It is prepared as an independent service for a later Pomegranate integration.

The current delivery does **not** include a portable Neo4j or Java runtime.
Use either an external Neo4j instance or the optional Docker Compose service.

## Included Capabilities

- Neo4j course graph for mechanical manufacturing process knowledge.
- Dynamic hierarchy loading:
  `Chapter -> Section -> Knowledge -> Concept`.
- `RELATED_TO` relationships between knowledge points.
- Chinese name/content search.
- Knowledge detail lookup.
- Static Cytoscape.js graph browser in `web/`.
- Course ZIP importer in `backend/scripts/import_process_graph.py`.
- Existing course data package:
  7 chapters, 47 sections, 283 knowledge points, 1449 concepts, and valid relationships.

Optional/partial capabilities are retained in code but are not required for the
Pomegranate MVP in this phase: generic manufacturing agent, SSE chat, vector
retrieval, GDS, and memory backend integration.

## Directory Layout

```text
backend/                         FastAPI service and tests
backend/app/routes.py             Safe course graph API routes
backend/scripts/import_process_graph.py
                                  Safe course ZIP importer
web/                              Static Cytoscape.js frontend
data/*Neo4j*.zip                  Course import package
cypher/                           Optional schema/GDS helpers
docker-compose.yml                Optional local Neo4j only
docker-compose.prod.yml           Optional Neo4j + backend
```

## Environment

Copy `.env.example` to `.env` and edit values:

```powershell
Copy-Item .env.example .env
```

Required values:

```env
MEMORY_BACKEND=bolt
NEO4J_URI=neo4j://localhost:7687
NEO4J_USERNAME=neo4j
NEO4J_PASSWORD=change-me
NEO4J_DATABASE=mechanical_process_graph
ENABLE_ADMIN_ROUTES=false
```

`ENABLE_ADMIN_ROUTES=false` keeps the generic `/api/cypher` route disabled.
Keep it disabled for desktop application use.

Neo4j Community installations usually expose only the default user database.
If you use Neo4j Community, run a **dedicated isolated container/volume** for
this course graph and use `NEO4J_DATABASE=neo4j` only in that isolated instance.

## Install

Using the repository scripts:

```powershell
.\setup.ps1
```

Or manually:

```powershell
cd backend
python -m venv ..\.venv
..\.venv\Scripts\python.exe -m pip install -e ".[dev]"
```

## Start Neo4j

Use an existing external Neo4j, or start the provided isolated Docker service:

```powershell
docker compose up -d
```

This compose file only starts Neo4j. It does not start a nonexistent Next.js
frontend.

## Import Course Data

The importer clears the target graph before importing. It is intentionally
blocked unless you explicitly confirm the reset and name the target database.

Dedicated database:

```powershell
.\seed.ps1 -Database mechanical_process_graph -ConfirmReset
```

Dedicated Neo4j Community container using the default `neo4j` database:

```powershell
.\seed.ps1 -Database neo4j -ConfirmReset -AllowDefaultDatabase
```

Do not point this importer at any firstwork/Pomegranate graph database.

## Start Backend and Web UI

```powershell
.\start.ps1
```

Then open:

- Graph UI: `http://127.0.0.1:8000`
- API docs: `http://127.0.0.1:8000/docs`
- Neo4j Browser: `http://127.0.0.1:7474`

The static web UI is served by FastAPI from `web/`.

## Safe Course API

These are the intended Pomegranate-facing endpoints for the next phase:

```text
GET  /api/process-graph/chapters
POST /api/process-graph/expand
GET  /api/process-graph/knowledge/{knowledge_id}
POST /api/process-graph/search
GET  /health
```

The following are not part of the desktop integration contract in this phase:

- `/api/cypher` generic Cypher endpoint.
- Admin import/reset actions.
- Unrestricted write APIs.
- Generic agent tools with write/admin permissions.

## Tests

```powershell
cd backend
python -m pytest tests/ -v
```

The unit tests mock Neo4j and cover the four core course graph endpoints:
chapters, expand, knowledge detail, and Chinese search.
