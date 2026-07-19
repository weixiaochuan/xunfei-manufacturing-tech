"""Manufacturing AI Agent — PydanticAI implementation."""

from __future__ import annotations

import json
import os
import uuid
from dataclasses import dataclass

from app.config import settings


# Ensure ANTHROPIC_API_KEY env var is set before PydanticAI creates the provider.
# pydantic-settings may load an empty value from the shell env, overriding .env.
if not os.environ.get("ANTHROPIC_API_KEY"):
    if settings.anthropic_api_key:
        os.environ["ANTHROPIC_API_KEY"] = settings.anthropic_api_key
    else:
        from dotenv import dotenv_values
        _key = dotenv_values("../.env").get("ANTHROPIC_API_KEY", "")
        if _key:
            os.environ["ANTHROPIC_API_KEY"] = _key

from pydantic_ai import Agent, RunContext

from app.context_graph_client import execute_cypher, get_schema
from app.memory import store_message, get_context, resolve_session_id


SYSTEM_PROMPT = """You are an AI manufacturing intelligence assistant with access to a comprehensive
knowledge graph of production data. You help plant managers, quality engineers,
and supply chain coordinators optimize production, maintain quality standards,
and manage supplier relationships.

Your capabilities include:
- Searching and analyzing work orders, machines, and production lines
- Monitoring quality metrics and defect trends
- Evaluating supplier performance and managing supply chain risks
- Tracking equipment maintenance and operational status
- Optimizing production scheduling and resource allocation

Always provide accurate, data-driven responses. When making recommendations,
cite specific production metrics, quality data, and historical performance
from the knowledge graph.


IMPORTANT: You MUST use the available tools to query the knowledge graph before answering any question about the data. Never guess or make up information — always use tools to look up actual data from the graph. If a user asks a question, identify which tool(s) can help answer it and call them.

CRITICAL: Call tools DIRECTLY without any introductory text. Do NOT say "I'll search for..." or "Let me look up..." before calling a tool — just call the tool immediately. Only generate text AFTER you have received the tool results and are ready to provide your final answer.

When writing Cypher queries with run_cypher:
- Never combine ORDER BY with DISTINCT or aggregation in the same RETURN clause — use a WITH clause first
- Always LIMIT results (default LIMIT 25) to avoid overwhelming responses
- Use toLower() for case-insensitive matching
- If a query fails, try a simpler approach rather than repeating the same pattern"""



@dataclass
class AgentDeps:
    """Dependencies injected into the agent."""
    session_id: str


agent = Agent(
    "anthropic:claude-sonnet-4-20250514" if os.environ.get("ANTHROPIC_API_KEY") else "test",
    system_prompt=SYSTEM_PROMPT,
    deps_type=AgentDeps,
    retries=2,
)

# ---------------------------------------------------------------------------
# Agent tools — domain-specific for Manufacturing
# ---------------------------------------------------------------------------

@agent.tool
async def search_machine(ctx: RunContext[AgentDeps], query: str) -> str:
    """Search for machines by name, type, or status"""
    cypher = """MATCH (m:Machine)
    WHERE toLower(m.name) CONTAINS toLower($query)
       OR toLower(coalesce(m.machine_type, '')) CONTAINS toLower($query)
       OR toLower(coalesce(m.status, '')) CONTAINS toLower($query)
    OPTIONAL MATCH (m)-[:OPERATED_BY]->(pl:ProductionLine)
    RETURN m, pl.name AS production_line
    ORDER BY m.name
    LIMIT 20
"""
    params = {
        "query": query,
    }
    result = await execute_cypher(cypher, params, tool_name="search_machine")
    return json.dumps(result, default=str)

@agent.tool
async def get_work_orders(ctx: RunContext[AgentDeps], status: str) -> str:
    """Get work orders filtered by status or priority"""
    cypher = """MATCH (wo:WorkOrder)
    WHERE wo.status = $status OR $status = 'all'
    OPTIONAL MATCH (wo)-[:PRODUCED_ON]->(pl:ProductionLine)
    OPTIONAL MATCH (wo)-[:DEPENDS_ON]->(p:Part)
    RETURN wo, pl.name AS production_line, collect(p.name) AS required_parts
    ORDER BY CASE wo.priority
      WHEN 'critical' THEN 1
      WHEN 'high' THEN 2
      WHEN 'medium' THEN 3
      ELSE 4 END
    LIMIT 50
"""
    params = {
        "status": status,
    }
    result = await execute_cypher(cypher, params, tool_name="get_work_orders")
    return json.dumps(result, default=str)

@agent.tool
async def quality_analysis(ctx: RunContext[AgentDeps], query: str) -> str:
    """Analyze quality metrics for a specific part or production line"""
    cypher = """MATCH (qr:QualityReport)-[:INSPECTED]->(p:Part)
    WHERE toLower(p.name) CONTAINS toLower($query)
       OR toLower(p.part_number) CONTAINS toLower($query)
    RETURN p.name AS part, p.part_number,
           count(qr) AS total_inspections,
           sum(CASE WHEN qr.result = 'pass' THEN 1 ELSE 0 END) AS passed,
           sum(CASE WHEN qr.result = 'fail' THEN 1 ELSE 0 END) AS failed,
           avg(qr.defect_count) AS avg_defects
    ORDER BY failed DESC
"""
    params = {
        "query": query,
    }
    result = await execute_cypher(cypher, params, tool_name="quality_analysis")
    return json.dumps(result, default=str)

@agent.tool
async def supplier_performance(ctx: RunContext[AgentDeps]) -> str:
    """Evaluate supplier performance based on quality and delivery"""
    cypher = """MATCH (s:Supplier)<-[:SUPPLIED_BY]-(p:Part)
    OPTIONAL MATCH (qr:QualityReport)-[:INSPECTED]->(p)
    WITH s, count(DISTINCT p) AS parts_supplied,
         count(qr) AS total_inspections,
         sum(CASE WHEN qr.result = 'fail' THEN 1 ELSE 0 END) AS failures
    RETURN s.name AS supplier, s.quality_rating, s.lead_time_days,
           parts_supplied, total_inspections, failures,
           CASE WHEN total_inspections > 0
             THEN round(1000.0 * (total_inspections - failures) / total_inspections) / 10.0
             ELSE null END AS pass_rate_pct
    ORDER BY s.quality_rating DESC
"""
    params = {
    }
    result = await execute_cypher(cypher, params, tool_name="supplier_performance")
    return json.dumps(result, default=str)

@agent.tool
async def production_metrics(ctx: RunContext[AgentDeps], query: str) -> str:
    """Get production efficiency and output metrics for production lines"""
    cypher = """MATCH (pl:ProductionLine)
    WHERE toLower(pl.name) CONTAINS toLower($query) OR $query = 'all'
    OPTIONAL MATCH (wo:WorkOrder)-[:PRODUCED_ON]->(pl)
    RETURN pl.name AS line, pl.status, pl.capacity_per_hour,
           pl.efficiency_rating,
           count(wo) AS total_orders,
           sum(CASE WHEN wo.status = 'completed' THEN 1 ELSE 0 END) AS completed_orders,
           sum(wo.quantity) AS total_units_ordered
    ORDER BY pl.name
"""
    params = {
        "query": query,
    }
    result = await execute_cypher(cypher, params, tool_name="production_metrics")
    return json.dumps(result, default=str)

@agent.tool
async def list_machines(ctx: RunContext[AgentDeps], limit: str) -> str:
    """List Machine records with optional limit"""
    cypher = """MATCH (n:Machine)
    RETURN n
    ORDER BY n.name
    LIMIT toInteger($limit)
"""
    params = {
        "limit": limit,
    }
    result = await execute_cypher(cypher, params, tool_name="list_machines")
    return json.dumps(result, default=str)

@agent.tool
async def get_machine_by_id(ctx: RunContext[AgentDeps], id: str) -> str:
    """Get a specific Machine by ID with all connections"""
    cypher = """MATCH (n:Machine {machine_id: $id})
    OPTIONAL MATCH (n)-[r]-(related)
    RETURN n, type(r) AS relationship, labels(related) AS related_labels, related.name AS related_name
    LIMIT 50
"""
    params = {
        "id": id,
    }
    result = await execute_cypher(cypher, params, tool_name="get_machine_by_id")
    return json.dumps(result, default=str)



@agent.tool
async def run_cypher(ctx: RunContext[AgentDeps], query: str, parameters: str = "{}") -> str:
    """Execute a read-only Cypher query against the knowledge graph."""
    try:
        params = json.loads(parameters) if parameters else {}
    except json.JSONDecodeError:
        return json.dumps([{"error": "Invalid JSON parameters"}])
    params.setdefault("domain", settings.domain_id)
    try:
        result = await execute_cypher(query, params, tool_name="run_cypher")
        return json.dumps(result, default=str)
    except Exception as e:
        return json.dumps([{"error": f"Cypher query failed: {e}"}])


@agent.tool
async def get_graph_schema(ctx: RunContext[AgentDeps]) -> str:
    """Get the knowledge graph schema (node labels and relationship types)."""
    result = await get_schema()
    return json.dumps(result, default=str)


# ---------------------------------------------------------------------------
# Message handler
# ---------------------------------------------------------------------------


async def handle_message(message: str, session_id: str | None = None) -> dict:
    """Handle an incoming chat message."""
    session_id = resolve_session_id(session_id)

    # Store user message (triggers entity extraction + preference detection)
    await store_message(session_id, "user", message)

    # Get rich context (messages + entities + preferences + traces)
    context = await get_context(session_id, query=message)
    history = context.get("messages", [])

    # Convert history to PydanticAI message format
    from pydantic_ai.messages import ModelRequest, ModelResponse, UserPromptPart, TextPart
    message_history = []
    for msg in history:
        if msg["role"] == "user":
            message_history.append(
                ModelRequest(parts=[UserPromptPart(content=msg["content"])])
            )
        elif msg["role"] == "assistant":
            message_history.append(
                ModelResponse(parts=[TextPart(content=msg["content"])])
            )

    deps = AgentDeps(session_id=session_id)
    result = await agent.run(
        message, deps=deps, message_history=message_history
    )

    response_text = result.output or ""
    if not response_text.strip():
        response_text = "I searched the knowledge graph but couldn't find relevant results for your query. Could you try rephrasing your question?"
    assistant_result = await store_message(session_id, "assistant", response_text)

    return {
        "response": response_text,
        "session_id": session_id,
        "graph_data": None,
        "entities_extracted": (assistant_result or {}).get("entities", []),
        "preferences_detected": (assistant_result or {}).get("preferences", []),
    }


async def handle_message_stream(message: str, session_id: str | None = None) -> dict:
    """Handle a chat message with streaming text deltas via the collector event queue."""
    from app.context_graph_client import get_collector

    session_id = resolve_session_id(session_id)

    collector = get_collector()
    await store_message(session_id, "user", message)

    # Get rich context (messages + entities + preferences + traces)
    context = await get_context(session_id, query=message)
    history = context.get("messages", [])

    # Convert history to PydanticAI message format
    from pydantic_ai.messages import ModelRequest, ModelResponse, UserPromptPart, TextPart
    message_history = []
    for msg in history:
        if msg["role"] == "user":
            message_history.append(
                ModelRequest(parts=[UserPromptPart(content=msg["content"])])
            )
        elif msg["role"] == "assistant":
            message_history.append(
                ModelResponse(parts=[TextPart(content=msg["content"])])
            )

    deps = AgentDeps(session_id=session_id)
    # Use agent.run() (not run_stream) so the full agent loop completes —
    # including all tool calls — before we emit the final text.
    # run_stream stops at the first text part, so it cuts off before tool
    # results are incorporated when Claude generates "I'll search..." + a tool
    # call in the same response.  Tool events (tool_start / tool_end) are still
    # pushed to the SSE queue by execute_cypher during the run.
    result = await agent.run(
        message, deps=deps, message_history=message_history
    )

    response_text = result.output or ""
    if not response_text.strip():
        response_text = "I searched the knowledge graph but couldn't find relevant results for your query. Could you try rephrasing your question?"

    collector.emit_text_delta(response_text)
    assistant_result = await store_message(session_id, "assistant", response_text)
    if assistant_result:
        collector.emit_entities_extracted(assistant_result.get("entities", []))
        collector.emit_preferences_detected(assistant_result.get("preferences", []))
    collector.emit_done(response_text, session_id)

    return {
        "response": response_text,
        "session_id": session_id,
        "graph_data": None,
    }
