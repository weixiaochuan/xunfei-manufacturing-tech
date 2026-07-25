-include .env
export

.PHONY: install start dev-backend seed test test-connection docker-up docker-down docker-prod-up docker-prod-down clean

BACKEND_PORT ?= 8000

install:
	cd backend && uv sync --extra dev

start dev-backend:
	cd backend && uv run uvicorn app.main:app --reload --port $${BACKEND_PORT:-8000}

seed:
	test -n "$$NEO4J_DATABASE" || (echo "Set NEO4J_DATABASE to an isolated database name before seeding."; exit 1)
	cd backend && uv run python scripts/import_process_graph.py --database $$NEO4J_DATABASE --confirm-reset

seed-default-dev:
	cd backend && uv run python scripts/import_process_graph.py --database neo4j --confirm-reset --allow-default-database

test-connection:
	cd backend && uv run python -c "import asyncio; from app.context_graph_client import connect_neo4j, close_neo4j; asyncio.run(connect_neo4j()); print('Neo4j connection successful'); asyncio.run(close_neo4j())"

docker-up:
	docker compose up -d

docker-down:
	docker compose down

docker-prod-up:
	docker compose -f docker-compose.prod.yml up --build -d

docker-prod-down:
	docker compose -f docker-compose.prod.yml down

test:
	cd backend && uv run python -m pytest tests/ -v

clean:
	rm -rf backend/__pycache__ backend/.venv
