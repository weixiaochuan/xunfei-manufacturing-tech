"""Tests for Manufacturing Context Graph API."""

import os
import pytest
from unittest.mock import AsyncMock, patch
from fastapi.testclient import TestClient

# Set placeholder keys before importing app modules so framework agents that
# validate API keys at module-level (e.g. PydanticAI) don't raise on import.
# These are never used — no real LLM calls happen in unit tests.
os.environ.setdefault("ANTHROPIC_API_KEY", "test-placeholder")
os.environ.setdefault("OPENAI_API_KEY", "test-placeholder")
os.environ.setdefault("GOOGLE_API_KEY", "test-placeholder")

from app.main import app


@pytest.fixture(autouse=True)
def mock_backend():
    """Mock the memory backend for all tests.

    Patches both the bolt-Neo4j path (connect_neo4j, vector index) and the
    NAMS path (connect_memory) so a single test file works regardless of
    which backend the generated project targets.
    """
    with patch("app.context_graph_client.connect_neo4j", new_callable=AsyncMock), \
         patch("app.context_graph_client.close_neo4j", new_callable=AsyncMock), \
         patch("app.main.is_connected", return_value=True), \
         patch("app.main.execute_cypher", new_callable=AsyncMock, return_value=[{"ok": 1}]), \
         patch("app.routes.is_connected", return_value=True), \
         patch("app.main.get_memory_status", return_value=True), \
         patch("app.memory.connect_memory", new_callable=AsyncMock), \
         patch("app.memory.close_memory", new_callable=AsyncMock), \
         patch("app.vector_client.create_vector_index", new_callable=AsyncMock):
        yield


client = TestClient(app)


def test_health():
    response = client.get("/health")
    assert response.status_code == 200
    data = response.json()
    assert data["status"] == "ok"
    assert data["domain"] == "manufacturing"



def test_scenarios():
    response = client.get("/api/scenarios")
    assert response.status_code == 200
    data = response.json()
    assert "domain" in data
    assert "scenarios" in data
    assert isinstance(data["scenarios"], list)


def test_process_graph_chapters():
    chapter = {
        "id": "CH_1",
        "name": "第一章 机械制造工艺概论",
        "labels": ["Entity", "Chapter"],
        "elementId": "chapter-1",
    }
    with patch("app.routes.execute_cypher", new_callable=AsyncMock, return_value=[{"n": chapter}]) as execute:
        response = client.get("/api/process-graph/chapters")

    assert response.status_code == 200
    assert response.json() == {"nodes": [chapter], "relationships": []}
    execute.assert_awaited_once()


def test_process_graph_expand_section():
    identity = [{"labels": ["Entity", "Chapter"]}]
    child = {
        "id": "SEC_1_1",
        "name": "第一节 概述",
        "labels": ["Entity", "Section"],
        "elementId": "section-1-1",
    }
    rel = {
        "elementId": "rel-1",
        "type": "HAS_SECTION",
        "startNodeElementId": "chapter-1",
        "endNodeElementId": "section-1-1",
    }
    with patch("app.routes.execute_cypher", new_callable=AsyncMock, side_effect=[identity, [{"m": child, "r": rel}]]) as execute:
        response = client.post("/api/process-graph/expand", json={"element_id": "CH_1"})

    assert response.status_code == 200
    assert response.json()["results"][0]["m"]["id"] == "SEC_1_1"
    assert execute.await_count == 2


def test_process_graph_knowledge_detail():
    detail = {
        "name": "定位基准",
        "content": "定位基准用于确定工件在夹具中的正确位置。",
        "knowledge_type": "definition",
        "chapter": "第三章 工艺规程设计",
        "section": "第二节 定位基准选择",
    }
    with patch("app.routes.execute_cypher", new_callable=AsyncMock, return_value=[detail]) as execute:
        response = client.get("/api/process-graph/knowledge/K_001")

    assert response.status_code == 200
    assert response.json()["name"] == "定位基准"
    assert "夹具" in response.json()["content"]
    execute.assert_awaited_once()


def test_process_graph_chinese_search():
    node = {
        "id": "K_001",
        "name": "定位基准",
        "content": "定位基准选择原则",
        "labels": ["Entity", "Knowledge"],
        "elementId": "knowledge-1",
    }
    with patch("app.routes.execute_cypher", new_callable=AsyncMock, return_value=[{"n": node}]) as execute:
        response = client.post("/api/process-graph/search", json={"query": "定位", "limit": 20})

    assert response.status_code == 200
    assert response.json() == {"nodes": [node], "relationships": []}
    execute.assert_awaited_once()


def test_admin_cypher_is_disabled_by_default():
    response = client.post("/api/cypher", json={"query": "MATCH (n) RETURN n LIMIT 1"})
    assert response.status_code == 404
